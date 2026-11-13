use std::error::Error as _;
use std::fmt;
use std::time::Duration;

use chigiri_rate::{
    FixedWindowPolicy, MonotonicTick, OperationCost, OperationCostError, QuotaKey, QuotaSubject,
    RateLimitDecision, RateLimitFuture, RateLimitProviderError, RateLimitRejectReason,
    RateLimitRequest, RateLimiter, RatePolicy, TokenBucketPolicy,
};

struct MinimalProvider;

impl RateLimiter for MinimalProvider {
    fn check_and_consume<'a>(&'a self, request: RateLimitRequest<'a>) -> RateLimitFuture<'a> {
        Box::pin(async move {
            RateLimitDecision::allowed(
                request,
                request.policy().limit() - request.cost().get(),
                MonotonicTick::from_nanos(1),
            )
            .map_err(RateLimitProviderError::new)
        })
    }
}

#[test]
fn rspi_02_trait_is_object_safe_and_returns_a_borrowed_send_future() {
    fn assert_provider(_: &(dyn RateLimiter + Send + Sync)) {}
    let provider = MinimalProvider;
    assert_provider(&provider);
    let key = key("identity");
    let request = RateLimitRequest::new(
        &key,
        OperationCost::one(),
        FixedWindowPolicy::new(10, Duration::from_secs(1))
            .unwrap()
            .into(),
    );
    fn assert_send<T: Send>(_: &T) {}
    let future = provider.check_and_consume(request);
    assert_send(&future);
}

#[test]
fn rspi_05_detects_cost_key_policy_and_decision_mismatches() {
    assert_eq!(OperationCost::new(0), Err(OperationCostError::Zero));
    assert_eq!(
        OperationCost::new(u64::MAX)
            .unwrap()
            .checked_add(OperationCost::one()),
        Err(OperationCostError::Overflow)
    );
    assert!(QuotaSubject::value(Vec::new()).is_err());
    assert!(QuotaKey::new(Vec::new(), QuotaSubject::Missing, None).is_err());
    assert!(TokenBucketPolicy::new(0, 1, Duration::from_secs(1)).is_err());
    assert!(FixedWindowPolicy::new(1, Duration::ZERO).is_err());
    let decision_key = key("decision");
    let decision_policy = FixedWindowPolicy::new(1, Duration::from_secs(1))
        .unwrap()
        .into();
    let decision_request =
        RateLimitRequest::new(&decision_key, OperationCost::one(), decision_policy);
    assert!(RateLimitDecision::allowed(decision_request, 2, MonotonicTick::from_nanos(0)).is_err());
    assert!(
        RateLimitDecision::rejected(
            decision_request,
            0,
            MonotonicTick::from_nanos(0),
            Some(Duration::ZERO),
            RateLimitRejectReason::QuotaExceeded,
        )
        .is_err()
    );
    assert!(
        RateLimitDecision::rejected(
            decision_request,
            0,
            MonotonicTick::from_nanos(0),
            None,
            RateLimitRejectReason::QuotaExceeded,
        )
        .is_err()
    );
}

#[test]
fn rspi_06_rejection_reasons_and_coordinate_snapshots_are_closed() {
    let key = key("reject");
    let policy = FixedWindowPolicy::new(10, Duration::from_secs(1))
        .unwrap()
        .into();
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy);
    for reason in [
        RateLimitRejectReason::KeyCapacityExhausted,
        RateLimitRejectReason::PolicyMismatch,
        RateLimitRejectReason::DynamicCostExceedsLimit,
        RateLimitRejectReason::ArithmeticOverflow,
    ] {
        let snapshot =
            RateLimitDecision::rejected(request, 0, MonotonicTick::from_nanos(0), None, reason)
                .unwrap()
                .snapshot();
        assert_eq!(snapshot.reject_reason(), Some(reason));
    }
}

#[test]
fn rspi_07_public_debug_and_provider_errors_redact_secret_and_quota_values() {
    let secret = "operator-secret-123";
    let key = key(secret);
    let policy: RatePolicy = TokenBucketPolicy::new(987_654, 32, Duration::from_secs(7))
        .unwrap()
        .into();
    let request = RateLimitRequest::new(&key, OperationCost::new(987_654).unwrap(), policy);
    let snapshot = RateLimitDecision::allowed(request, 123_456, MonotonicTick::from_nanos(777))
        .unwrap()
        .snapshot();
    let cost = request.cost();
    for debug in [
        format!("{key:?}"),
        format!("{policy:?}"),
        format!("{snapshot:?}"),
        format!("{cost:?}"),
        format!("{:?}", MonotonicTick::from_nanos(123_456)),
        format!("{request:?}"),
    ] {
        assert!(!debug.contains(secret));
        assert!(!debug.contains("987654"));
        assert!(!debug.contains("123456"));
    }
    let error = RateLimitProviderError::new(SecretError(secret));
    assert!(!format!("{error}").contains(secret));
    assert!(!format!("{error:?}").contains(secret));
    assert!(error.source().unwrap().to_string().contains(secret));
}

fn key(subject: &str) -> QuotaKey {
    QuotaKey::new(
        b"test".to_vec(),
        QuotaSubject::value(subject.as_bytes().to_vec()).unwrap(),
        None,
    )
    .unwrap()
}

#[derive(Debug)]
struct SecretError(&'static str);

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SecretError {}
