//! Verify that an already-built schema input uses only the endpoint draft's component graph.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::support::{execute, resolvers, schema_builder};

#[test]
fn built_schema_input_does_not_inherit_the_source_plugin() {
    let source_calls = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .plugin(CountRequests(Arc::clone(&source_calls)))
        .build()
        .unwrap();
    let endpoint = chigiri::Endpoint::builder(schema)
        .without_context()
        .build()
        .unwrap();

    execute(&endpoint, "{ greeting }");
    assert_eq!(source_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn built_schema_supports_component_set_and_clear_operations() {
    let source_calls = Arc::new(AtomicUsize::new(0));
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .plugin(CountRequests(Arc::clone(&source_calls)))
        .build()
        .unwrap();
    let replacement: Arc<dyn chigiri::Plugin<()>> =
        Arc::new(CountRequests(Arc::clone(&replacement_calls)));
    let endpoint = chigiri::Endpoint::builder(schema)
        .inject(|components| {
            components.plugins().set_plugins(vec![replacement]);
        })
        .without_context()
        .build()
        .unwrap();
    execute(&endpoint, "{ greeting }");
    assert_eq!(source_calls.load(Ordering::SeqCst), 0);
    assert_eq!(replacement_calls.load(Ordering::SeqCst), 1);

    let source_calls = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .plugin(CountRequests(Arc::clone(&source_calls)))
        .build()
        .unwrap();
    let endpoint = chigiri::Endpoint::builder(schema)
        .inject(|components| {
            components.plugins().clear_plugins();
        })
        .without_context()
        .build()
        .unwrap();
    execute(&endpoint, "{ greeting }");
    assert_eq!(source_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn built_schema_input_removes_the_sampler_and_observer_from_source() {
    let samples = Arc::new(AtomicUsize::new(0));
    let lifecycle = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .sampler(CountSamples(Arc::clone(&samples)))
        .lifecycle_observer(CountLifecycle(Arc::clone(&lifecycle)))
        .field_instrumentation(true)
        .build()
        .unwrap();
    let source_build_events = lifecycle.load(Ordering::SeqCst);
    let endpoint = chigiri::Endpoint::builder(schema)
        .without_context()
        .build()
        .unwrap();

    execute(&endpoint, "{ greeting }");
    assert_eq!(samples.load(Ordering::SeqCst), 0);
    assert_eq!(lifecycle.load(Ordering::SeqCst), source_build_events);
}

#[test]
fn built_schema_input_keeps_the_catalog_and_replaces_the_error_observer() {
    let source_errors = Arc::new(AtomicUsize::new(0));
    let endpoint_errors = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .catalog_revision("catalog-built-1")
        .error_observer(CountErrors(Arc::clone(&source_errors)))
        .build()
        .unwrap();
    let endpoint = chigiri::Endpoint::builder(schema)
        .inject(|components| {
            components.plugins().push_plugin(InternalFieldFailure);
            components
                .observability()
                .error_observer(CountErrors(Arc::clone(&endpoint_errors)));
        })
        .without_context()
        .build()
        .unwrap();

    let response = execute(&endpoint, "{ greeting }");
    assert_eq!(response.errors.len(), 1);
    assert_eq!(source_errors.load(Ordering::SeqCst), 0);
    assert_eq!(endpoint_errors.load(Ordering::SeqCst), 1);
    assert_eq!(
        endpoint.schema_info().catalog_revision(),
        Some("catalog-built-1")
    );
}

#[test]
fn generated_and_built_schema_inputs_produce_the_same_final_graph() {
    let generated_plugin = Arc::new(AtomicUsize::new(0));
    let generated_samples = Arc::new(AtomicUsize::new(0));
    let generated_lifecycle = Arc::new(AtomicUsize::new(0));
    let built_plugin = Arc::new(AtomicUsize::new(0));
    let built_samples = Arc::new(AtomicUsize::new(0));
    let built_lifecycle = Arc::new(AtomicUsize::new(0));
    let generated = configured_endpoint(
        resolvers(),
        Arc::clone(&generated_plugin),
        Arc::clone(&generated_samples),
        Arc::clone(&generated_lifecycle),
    );
    let built = configured_endpoint(
        schema_builder().build().unwrap(),
        Arc::clone(&built_plugin),
        Arc::clone(&built_samples),
        Arc::clone(&built_lifecycle),
    );

    assert_eq!(
        execute(&generated, "{ greeting }"),
        execute(&built, "{ greeting }")
    );
    assert_eq!(generated.configuration(), built.configuration());
    assert_eq!(generated_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(built_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(generated_samples.load(Ordering::SeqCst), 1);
    assert_eq!(built_samples.load(Ordering::SeqCst), 1);
    assert_eq!(
        generated_lifecycle.load(Ordering::SeqCst),
        built_lifecycle.load(Ordering::SeqCst)
    );
}

fn configured_endpoint(
    input: impl chigiri::IntoEndpointInput<()>,
    plugin: Arc<AtomicUsize>,
    samples: Arc<AtomicUsize>,
    lifecycle: Arc<AtomicUsize>,
) -> chigiri::Endpoint<()> {
    chigiri::Endpoint::builder(input)
        .body_limit(4_096)
        .inject(|components| {
            components.plugins().push_plugin(CountRequests(plugin));
            components
                .observability()
                .sampler(CountSamples(samples))
                .lifecycle_observer(CountLifecycle(lifecycle));
        })
        .without_context()
        .build()
        .unwrap()
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

struct CountSamples(Arc<AtomicUsize>);

impl chigiri::Sampler for CountSamples {
    fn sample(&self, _: chigiri::RequestIdentity<'_>) -> chigiri::SamplingDecision {
        self.0.fetch_add(1, Ordering::SeqCst);
        chigiri::SamplingDecision::Record
    }
}

struct CountLifecycle(Arc<AtomicUsize>);

impl chigiri::LifecycleObserver for CountLifecycle {
    fn observe(&self, _: chigiri::LifecycleEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct CountErrors(Arc<AtomicUsize>);

impl chigiri::ErrorObserver for CountErrors {
    fn observe_error(&self, _: chigiri::ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct InternalFieldFailure;

impl chigiri::Plugin<()> for InternalFieldFailure {
    fn field<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: chigiri::FieldNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::FieldResult<chigiri::__private::ResolvedValue>> {
        Box::pin(async { Err(chigiri::FieldError::internal(SyntheticFailure)) })
    }
}

#[derive(Debug)]
struct SyntheticFailure;

impl std::fmt::Display for SyntheticFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("synthetic built schema failure")
    }
}

impl std::error::Error for SyntheticFailure {}
