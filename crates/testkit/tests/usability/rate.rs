//! Test official rate limiters, timeouts, redaction, and Stage A/B coverage.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chigiri::spi::{ApplicationMetadata, RequestHeader};

use super::rate_support::{
    BypassOperation, CountingLimiter, ImmediateDeadline, OperationKey, PendingLimiter,
    ProviderDeadline, TransportKey, code, key, rate_policy, transport_builder,
};
use super::support::{NeverDeadline, block_on, execute, resolvers, schema_builder};

#[test]
fn limit_07_failure_policy_default_and_override() {
    let default = transport_builder().build().unwrap();
    fn assert_observer_binding(_: &impl chigiri::rate::RateLimitObserverBinding) {}
    assert_observer_binding(&default);
    assert_eq!(
        default.failure_policy(),
        chigiri::rate::ProviderFailurePolicy::FailClosed
    );
    assert_eq!(default.provider_timeout(), Duration::from_millis(500));

    let overridden = transport_builder()
        .failure_policy(chigiri::rate::ProviderFailurePolicy::FailOpen)
        .provider_timeout(Duration::from_millis(77))
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        overridden.failure_policy(),
        chigiri::rate::ProviderFailurePolicy::FailOpen
    );
    assert_eq!(overridden.provider_timeout(), Duration::from_millis(77));
    assert!(
        chigiri::rate::TimedRateLimiter::new(
            PendingLimiter(Arc::new(AtomicBool::new(false))),
            ImmediateDeadline,
            Duration::MAX,
        )
        .is_err()
    );

    let dropped = Arc::new(AtomicBool::new(false));
    let stage_b = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        PendingLimiter(Arc::clone(&dropped)),
        rate_policy(10),
        chigiri::rate::ProviderFailurePolicy::FailOpen,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap()
    .with_provider_timeout(Duration::from_millis(1))
    .unwrap();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.rate_limit().stage_b_provider(stage_b);
        })
        .without_context()
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(
        chigiri::GraphQlRequest::new("{ greeting }"),
        (),
        &ProviderDeadline,
    ));
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn limit_08_timeout_and_cancel_drop_provider() {
    let dropped = Arc::new(AtomicBool::new(false));
    let limiter = chigiri::rate::TimedRateLimiter::new(
        PendingLimiter(Arc::clone(&dropped)),
        ImmediateDeadline,
        Duration::from_millis(1),
    )
    .unwrap();
    let key = key("pending");
    let policy = chigiri::rate::TokenBucketPolicy::new(1, 1, Duration::from_secs(1)).unwrap();
    let result = block_on(chigiri::rate::RateLimiter::check_and_consume(
        &limiter,
        chigiri::rate::RateLimitRequest::new(
            &key,
            chigiri::rate::OperationCost::one(),
            policy.into(),
        ),
    ));
    assert!(result.is_err());
    assert!(dropped.load(Ordering::SeqCst));

    let stage_a_dropped = Arc::new(AtomicBool::new(false));
    let stage_a = chigiri::rate::StageARateLimit::new(
        TransportKey,
        PendingLimiter(Arc::clone(&stage_a_dropped)),
        rate_policy(10),
        chigiri::rate::ProviderFailurePolicy::FailClosed,
    )
    .with_provider_timeout(Duration::from_millis(1))
    .unwrap();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.rate_limit().stage_a_provider(stage_a);
        })
        .without_context()
        .build()
        .unwrap();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let metadata = ApplicationMetadata::default();
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let response = block_on(endpoint.handle(
        request,
        chigiri_http::BufferedBody::new(br#"{"query":"{ greeting }"}"#.to_vec()),
        &ProviderDeadline,
    ));
    assert_eq!(response.status, 503);
    assert!(stage_a_dropped.load(Ordering::SeqCst));

    let stage_b_dropped = Arc::new(AtomicBool::new(false));
    let stage_b = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        PendingLimiter(Arc::clone(&stage_b_dropped)),
        rate_policy(10),
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap()
    .with_provider_timeout(Duration::from_millis(1))
    .unwrap();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.rate_limit().stage_b_provider(stage_b);
        })
        .without_context()
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(
        chigiri::GraphQlRequest::new("{ greeting }"),
        (),
        &ProviderDeadline,
    ));
    assert_eq!(code(&response), Some("RATE_LIMIT_UNAVAILABLE"));
    assert!(stage_b_dropped.load(Ordering::SeqCst));
}

#[test]
fn limit_09_preserves_stage_a_stage_b_and_rollback_contract() {
    let stage_a_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stage_b_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let policy = rate_policy(10);
    let stage_a = chigiri::rate::StageARateLimit::new(
        TransportKey,
        CountingLimiter::allow(Arc::clone(&stage_a_calls)),
        policy,
        chigiri::rate::ProviderFailurePolicy::FailClosed,
    );
    let stage_b = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        CountingLimiter::reject(Arc::clone(&stage_b_calls)),
        policy,
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .rate_limit()
                .stage_a_provider(stage_a)
                .stage_b_provider(stage_b);
        })
        .without_context()
        .build()
        .unwrap();

    let direct = execute(&endpoint, "{ greeting }");
    assert_eq!(code(&direct), Some("RATE_LIMITED"));
    assert_eq!(stage_a_calls.load(Ordering::SeqCst), 0);
    assert_eq!(stage_b_calls.load(Ordering::SeqCst), 1);

    let headers = [RequestHeader::new("content-type", b"application/json")];
    let metadata = ApplicationMetadata::default();
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let response = block_on(endpoint.handle(
        request,
        chigiri_http::BufferedBody::new(br#"{"query":"{ greeting }"}"#.to_vec()),
        &NeverDeadline,
    ));
    assert_eq!(response.status, 429);
    assert_eq!(stage_a_calls.load(Ordering::SeqCst), 1);
    assert_eq!(stage_b_calls.load(Ordering::SeqCst), 2);

    let rebuilt_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let built_schema = schema_builder().plugin(BypassOperation).build().unwrap();
    let rebuilt_stage = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        CountingLimiter::reject(Arc::clone(&rebuilt_calls)),
        policy,
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap();
    let rebuilt = chigiri::Endpoint::builder(built_schema)
        .inject(|components| {
            components.rate_limit().stage_b_provider(rebuilt_stage);
        })
        .without_context()
        .build()
        .unwrap();
    let response = execute(&rebuilt, "{ greeting }");
    assert_eq!(code(&response), Some("RATE_LIMITED"));
    assert_eq!(rebuilt_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn limit_10_convenience_injection_uses_the_same_precedence_as_explicit_stages() {
    let sugar = chigiri::Endpoint::builder(resolvers())
        .rate_limit(transport_builder().build().unwrap())
        .without_context()
        .build()
        .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .rate_limit()
                .stage_a(transport_builder().build().unwrap());
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        sugar.configuration().stage_a_rate_limit(),
        injected.configuration().stage_a_rate_limit()
    );
    assert_eq!(
        *sugar
            .configuration()
            .transport_admission_timeout()
            .effective(),
        Duration::from_secs(30)
    );

    let replaced = chigiri::Endpoint::builder(resolvers())
        .rate_limit(transport_builder().build().unwrap())
        .inject(|components| {
            components.rate_limit().stage_a(
                chigiri::rate::transport(TransportKey)
                    .fixed_window()
                    .limit(99)
                    .unwrap()
                    .window(Duration::from_secs(60))
                    .unwrap()
                    .build()
                    .unwrap(),
            );
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        replaced
            .configuration()
            .stage_a_rate_limit()
            .unwrap()
            .policy()
            .limit(),
        99
    );
}

#[test]
fn limit_11_shows_unconfigured_coverage_and_structural_limit() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        endpoint.configuration().rate_limit_coverage(),
        chigiri::RateLimitCoverage::Unconfigured
    );
    assert_ne!(
        endpoint
            .configuration()
            .execution_limits()
            .effective()
            .max_output_nodes,
        usize::MAX
    );
}

#[test]
fn limit_12_redacts_quota_and_provider_information() {
    let key = key("credential-private-tenant");
    let provider_error = chigiri::rate::RateLimitProviderError::new(std::io::Error::other(
        "redis://private-provider",
    ));
    assert!(!format!("{key:?}").contains("credential-private-tenant"));
    assert!(!format!("{provider_error:?}").contains("redis://private-provider"));
    assert!(!provider_error.to_string().contains("private-provider"));

    let revision = chigiri::rate::PolicyRevision::new(9_876_543).unwrap();
    let token = chigiri::rate::TokenBucketPolicy::new(
        7_654_321,
        1_234_567,
        Duration::from_nanos(8_765_432),
    )
    .unwrap()
    .with_revision(revision);
    let request = chigiri::rate::RateLimitRequest::new(
        &key,
        chigiri::rate::OperationCost::one(),
        token.into(),
    );
    let decision = chigiri::rate::RateLimitDecision::rejected(
        request,
        1_234_567,
        chigiri::rate::MonotonicTick::from_nanos(6_543_210),
        Some(Duration::from_nanos(5_432_109)),
        chigiri::rate::RateLimitRejectReason::QuotaExceeded,
    )
    .unwrap();
    let snapshot = decision.snapshot();
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .rate_limit(
            chigiri::rate::transport(TransportKey)
                .token_bucket()
                .capacity(7_654_321)
                .unwrap()
                .refill(1_234_567, Duration::from_nanos(8_765_432))
                .unwrap()
                .provider_timeout(Duration::from_nanos(5_432_109))
                .unwrap()
                .build()
                .unwrap(),
        )
        .without_context()
        .build()
        .unwrap();
    let debug = format!(
        "{token:?} {:?} {snapshot:?} {decision:?} {:?}",
        chigiri::rate::RatePolicy::from(token),
        endpoint.configuration()
    );
    for secret in [
        "credential-private-tenant",
        "TransportKey",
        "7654321",
        "1234567",
        "8765432",
        "9876543",
        "6543210",
        "5432109",
    ] {
        assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
    }
}

#[test]
fn limit_15_separates_query_concurrency_from_time_quota() {
    let rate = concat!(
        include_str!("../../../policy/src/lib.rs"),
        include_str!("../../../policy/src/stage_a.rs"),
        include_str!("../../../policy/src/stage_b.rs"),
    );
    let limits = chigiri::ExecutionLimits::production();
    assert_eq!(limits.max_concurrent_fields, 64);
    assert!(!rate.contains("release_permit"));
    assert!(!rate.contains("concurrent_fields"));
}
