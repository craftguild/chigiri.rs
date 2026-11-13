//! Verify that convenience APIs and typed injection edit the same draft graph.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::rate::{OperationQuotaExtractor, TransportQuotaExtractor};

use super::injection_support::{FixedSource, execute_http};
use super::support::{execute, resolvers};

#[test]
fn inj_01_matches_typed_injection() {
    let mut limits = chigiri::ExecutionLimits::production();
    limits.max_aliases = 12;
    let cache = chigiri::DocumentCacheLimits {
        max_entries: 12,
        max_query_bytes: 4_096,
    };
    let sugar = chigiri::Endpoint::builder(resolvers())
        .execution_limits(limits)
        .document_cache(cache)
        .body_limit(4_096)
        .response_byte_limit(8_192)
        .execution_timeout(Duration::from_secs(3))
        .without_context()
        .build()
        .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.execution().limits(limits).document_cache(cache);
            components
                .http()
                .body_limit(4_096)
                .response_byte_limit(8_192)
                .execution_timeout(Duration::from_secs(3));
        })
        .without_context()
        .build()
        .unwrap();

    assert_eq!(sugar.configuration(), injected.configuration());
    assert_eq!(
        execute(&sugar, "{ greeting }"),
        execute(&injected, "{ greeting }")
    );

    let sugar_calls = Arc::new(AtomicUsize::new(0));
    let injected_calls = Arc::new(AtomicUsize::new(0));
    let sugar = chigiri::Endpoint::builder(resolvers())
        .operation_source(FixedSource)
        .push_plugin(CountRequests(Arc::clone(&sugar_calls)))
        .rate_limit(transport_stage(10))
        .without_context()
        .build()
        .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.operation().source(FixedSource);
            components
                .plugins()
                .push_plugin(CountRequests(Arc::clone(&injected_calls)));
            components.rate_limit().stage_a(transport_stage(10));
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(execute_http(&sugar), execute_http(&injected));
    assert_eq!(sugar_calls.load(Ordering::SeqCst), 1);
    assert_eq!(injected_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        sugar.configuration().stage_a_rate_limit(),
        injected.configuration().stage_a_rate_limit()
    );
}

#[test]
fn inj_02_single_slot_uses_last_call() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .body_limit(2_048)
        .inject(|components| {
            components.http().body_limit(4_096);
        })
        .body_limit(8_192)
        .without_context()
        .build()
        .unwrap();
    assert_eq!(*endpoint.configuration().body_limit().effective(), 8_192);
    assert_eq!(
        endpoint.configuration().body_limit().origin(),
        chigiri::ConfigurationOrigin::User
    );
}

#[test]
fn inj_03_stages_coexist_and_only_replace_the_same_stage() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .rate_limit(transport_stage(10))
        .inject(|components| {
            components.rate_limit().stage_b(operation_stage(20));
            components.rate_limit().stage_a(transport_stage(30));
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        endpoint.configuration().rate_limit_coverage(),
        chigiri::RateLimitCoverage::Both
    );
    assert_eq!(
        endpoint
            .configuration()
            .stage_a_rate_limit()
            .unwrap()
            .policy()
            .limit(),
        30
    );
    assert_eq!(
        endpoint
            .configuration()
            .stage_b_rate_limit()
            .unwrap()
            .policy()
            .limit(),
        20
    );
}

#[test]
fn inj_04_multiple_injections_edit_the_same_graph() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.http().body_limit(2_048);
        })
        .inject(|components| {
            components.http().body_limit(4_096);
            components
                .execution()
                .limits(chigiri::ExecutionLimits::production());
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(*endpoint.configuration().body_limit().effective(), 4_096);
}

#[test]
fn inj_05_distinguishes_plugin_push_set_and_clear() {
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let replacement = Arc::new(AtomicUsize::new(0));
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .push_plugin(CountRequests(Arc::clone(&first)))
        .inject(|components| {
            components
                .plugins()
                .push_plugin(CountRequests(Arc::clone(&second)));
        })
        .without_context()
        .build()
        .unwrap();
    execute(&endpoint, "{ greeting }");
    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(second.load(Ordering::SeqCst), 1);

    let plugin: Arc<dyn chigiri::Plugin<()>> = Arc::new(CountRequests(Arc::clone(&replacement)));
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .push_plugin(CountRequests(Arc::clone(&first)))
        .inject(|components| {
            components.plugins().set_plugins(vec![plugin]);
        })
        .without_context()
        .build()
        .unwrap();
    execute(&endpoint, "{ greeting }");
    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(replacement.load(Ordering::SeqCst), 1);

    let endpoint = chigiri::Endpoint::builder(resolvers())
        .push_plugin(CountRequests(Arc::clone(&first)))
        .inject(|components| {
            components.plugins().clear_plugins();
        })
        .without_context()
        .build()
        .unwrap();
    execute(&endpoint, "{ greeting }");
    assert_eq!(first.load(Ordering::SeqCst), 1);
}

#[test]
fn inj_06_cross_validates_only_the_final_graph() {
    let repaired = chigiri::Endpoint::builder(resolvers())
        .execution_timeout(Duration::ZERO)
        .inject(|components| {
            components.http().execution_timeout(Duration::from_secs(1));
        })
        .without_context()
        .build();
    assert!(repaired.is_ok());
    let invalid = chigiri::Endpoint::builder(resolvers())
        .execution_timeout(Duration::ZERO)
        .without_context()
        .build()
        .err()
        .unwrap();
    let chigiri::EndpointBuildError::InvalidConfiguration(diagnostic) = invalid else {
        panic!("expected a schema composition diagnostic");
    };
    assert_eq!(
        diagnostic.slot(),
        chigiri::ConfigurationSlot::ExecutionTimeout
    );
    assert_eq!(diagnostic.origin(), chigiri::ConfigurationOrigin::User);
    assert_eq!(
        diagnostic.violation(),
        chigiri::ConfigurationViolation::ZeroDuration
    );
}

#[test]
fn inj_09_standard_setters_cannot_weaken_protection() {
    assert!(
        chigiri::Endpoint::builder(resolvers())
            .body_limit(0)
            .without_context()
            .build()
            .is_err()
    );
    assert!(
        chigiri::Endpoint::builder(resolvers())
            .response_byte_limit(0)
            .without_context()
            .build()
            .is_err()
    );
}

#[test]
fn inj_10_effective_config_is_redacted_and_shows_origin() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .body_limit(7_777)
        .inject(|components| {
            components
                .execution()
                .dangerously_enable_introspection("ticket-USABILITY-10");
        })
        .without_context()
        .build()
        .unwrap();
    let config = endpoint.configuration();
    assert_eq!(
        config.body_limit().origin(),
        chigiri::ConfigurationOrigin::User
    );
    assert_eq!(config.body_limit().baseline(), &(1024 * 1024));
    let debug = format!("{config:?}");
    assert!(!debug.contains("raw query"));
    assert!(!debug.contains("credential"));
    assert!(!debug.contains("QuotaKey"));
    assert!(!debug.contains("ticket-USABILITY-10"));
    assert_eq!(config.safety_overrides()[0].reason(), "ticket-USABILITY-10");
}

struct CountRequests(Arc<AtomicUsize>);

impl chigiri::Plugin<()> for CountRequests {
    fn request<'a>(
        &'a self,
        context: chigiri::RequestContext<'a, ()>,
        next: chigiri::RequestNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }
}

struct TransportKey;

impl TransportQuotaExtractor for TransportKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        key().map_err(chigiri::spi::AdmissionError::new)
    }
}

struct OperationKey;

impl OperationQuotaExtractor<()> for OperationKey {
    fn extract(
        &self,
        _: &chigiri::OperationContext<'_, ()>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::GraphQlError> {
        key().map_err(|_| chigiri::GraphQlError::new("quota key failed"))
    }
}

fn transport_stage(
    limit: u64,
) -> chigiri::rate::TransportRateStage<chigiri::rate::StageARateLimit<TransportKey>> {
    chigiri::rate::transport(TransportKey)
        .fixed_window()
        .limit(limit)
        .unwrap()
        .window(Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap()
}

fn operation_stage(
    limit: u64,
) -> chigiri::rate::OperationRateStage<chigiri::rate::OperationRateLimit<OperationKey>> {
    chigiri::rate::operation(OperationKey)
        .sliding_window_counter()
        .limit(limit)
        .unwrap()
        .window(Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap()
}

fn key() -> Result<chigiri::rate::QuotaKey, chigiri::rate::QuotaKeyError> {
    chigiri::rate::QuotaKey::new(
        b"private-principal".to_vec(),
        chigiri::rate::QuotaSubject::Missing,
        None,
    )
}
