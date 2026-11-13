//! Test the single endpoint builder and automatic safe baseline.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::spi::{ApplicationMetadata, RequestHeader};

use super::support::{NeverDeadline, block_on, endpoint, execute, resolvers, schema};

#[test]
fn api_01_single_builder_reaches_query_mutation_get() {
    let endpoint = endpoint();
    let query = execute(&endpoint, "{ greeting item { id name } }");
    let mutation = execute(&endpoint, r#"mutation { rename(name: "new") }"#);
    assert!(query.errors.is_empty(), "{:?}", query.errors);
    assert!(mutation.errors.is_empty(), "{:?}", mutation.errors);

    let headers = [
        RequestHeader::new("accept", b"application/graphql-response+json"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = chigiri::TransportRequest::new(
        "GET",
        "/graphql",
        Some("query=%7B%20greeting%20%7D"),
        &headers,
        None,
        &metadata,
    );
    let response =
        block_on(endpoint.handle(request, chigiri_http::BufferedBody::empty(), &NeverDeadline));
    assert_eq!(response.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["data"]["greeting"],
        "hello"
    );
}

#[test]
fn api_02_applies_safe_baseline_without_additional_calls() {
    let endpoint = endpoint();
    let config = endpoint.configuration();
    let limits = config.execution_limits().effective();
    assert_eq!(limits.max_document_bytes, 256 * 1024);
    assert_eq!(limits.max_operation_depth, 32);
    assert_eq!(limits.max_expanded_fields, 1_000);
    assert_eq!(limits.max_concurrent_fields, 64);
    assert_eq!(limits.max_list_items_per_field, 1_000);
    assert_eq!(limits.max_output_nodes, 100_000);
    assert_eq!(config.document_cache().effective().max_entries, 1_024);
    assert!(!*config.introspection_enabled().effective());
    assert_eq!(*config.body_limit().effective(), 1024 * 1024);
    assert_eq!(*config.response_byte_limit().effective(), 8 * 1024 * 1024);
    assert_eq!(
        *config.execution_timeout().effective(),
        Duration::from_secs(30)
    );
    assert!(config.safety_overrides().is_empty());
}

#[test]
fn api_03_accepts_resolvers_and_schema_on_the_same_builder() {
    let generated = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .build()
        .unwrap();
    let built = chigiri::Endpoint::builder(schema())
        .without_context()
        .build()
        .unwrap();
    let request = "{ greeting }";
    assert_eq!(execute(&generated, request), execute(&built, request));
    assert_eq!(generated.schema_info(), built.schema_info());

    let observed = Arc::new(AtomicUsize::new(0));
    let configured = super::support::schema_builder()
        .field_instrumentation(true)
        .lifecycle_observer(CountLifecycle(Arc::clone(&observed)))
        .build()
        .unwrap();
    let before = observed.load(Ordering::SeqCst);
    let reset = chigiri::Endpoint::builder(configured)
        .without_context()
        .build()
        .unwrap();
    execute(&reset, request);
    assert_eq!(observed.load(Ordering::SeqCst), before);
}

#[test]
fn api_06_validates_request_id_configuration_at_build_and_request_time() {
    let identity = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .observability()
                .request_id_factory(FailingRequestIds);
        })
        .without_context()
        .build()
        .err()
        .unwrap();
    assert_eq!(
        identity.to_string(),
        "request identity initialization failed"
    );
    assert!(!format!("{identity:?}").contains("private entropy failure"));
    assert!(std::error::Error::source(&identity).is_none());
    assert!(matches!(
        identity,
        chigiri::EndpointBuildError::IdentityInitialization
    ));

    let invalid = chigiri::Endpoint::builder(resolvers())
        .body_limit(0)
        .without_context()
        .build()
        .err()
        .unwrap();
    let chigiri::EndpointBuildError::InvalidConfiguration(diagnostic) = invalid else {
        panic!("expected a schema composition diagnostic");
    };
    assert_eq!(
        diagnostic.slot(),
        chigiri::ConfigurationSlot::RequestBodyByteLimit
    );
    assert_eq!(diagnostic.origin(), chigiri::ConfigurationOrigin::User);
    assert_eq!(
        diagnostic.violation(),
        chigiri::ConfigurationViolation::OutsideHardBound
    );

    let cache_disabled = chigiri::Endpoint::builder(resolvers())
        .document_cache(chigiri::DocumentCacheLimits::disabled())
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        cache_disabled.configuration().document_cache().effective(),
        &chigiri::DocumentCacheLimits::disabled()
    );
    let half_disabled = chigiri::Endpoint::builder(resolvers())
        .document_cache(chigiri::DocumentCacheLimits {
            max_entries: 0,
            max_query_bytes: 1,
        })
        .without_context()
        .build();
    assert!(half_disabled.is_err());
}

#[test]
fn api_07_rate_keeps_structural_limits_when_unconfigured() {
    let endpoint = endpoint();
    assert_eq!(
        endpoint.configuration().rate_limit_coverage(),
        chigiri::RateLimitCoverage::Unconfigured
    );
    assert_ne!(
        endpoint
            .configuration()
            .execution_limits()
            .effective()
            .max_document_bytes,
        usize::MAX
    );
    assert_ne!(
        *endpoint.configuration().body_limit().effective(),
        usize::MAX
    );
    assert_ne!(
        *endpoint.configuration().overall_timeout().effective(),
        Duration::MAX
    );
}

#[test]
fn api_11_only_dangerous_api_can_weaken_the_baseline() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .execution()
                .dangerously_enable_introspection("operator-approved schema tooling");
            components
                .http()
                .dangerously_disable_csrf_guard("non-browser private transport");
        })
        .without_context()
        .build()
        .unwrap();
    assert!(*endpoint.configuration().introspection_enabled().effective());
    assert_eq!(endpoint.configuration().safety_overrides().len(), 2);
    assert!(
        chigiri::Endpoint::builder(resolvers())
            .execution_limits(chigiri::ExecutionLimits::unbounded())
            .without_context()
            .build()
            .is_err()
    );
}

struct FailingRequestIds;

struct CountLifecycle(Arc<AtomicUsize>);

impl chigiri::LifecycleObserver for CountLifecycle {
    fn observe(&self, event: chigiri::LifecycleEvent<'_>) {
        if event.component() == "field" {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl chigiri::RequestIdFactory for FailingRequestIds {
    fn create(&self) -> Result<chigiri::RequestId, chigiri::RequestIdFactoryError> {
        Err(chigiri::RequestIdFactoryError::new(std::io::Error::other(
            "private entropy failure",
        )))
    }
}
