//! Verifies that an inherited operation plugin and a replacement Stage B do not both run.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::rate_support::{CountingLimiter, OperationKey, code, rate_policy};
use super::support::{execute, resolvers, schema_builder};

#[test]
fn built_schema_input_replaces_inherited_operation_plugin_with_stage_b() {
    let inherited_calls = Arc::new(AtomicUsize::new(0));
    let stage_b_calls = Arc::new(AtomicUsize::new(0));
    let schema = schema_builder()
        .plugin(CountOperations(Arc::clone(&inherited_calls)))
        .build()
        .unwrap();
    let stage_b = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        CountingLimiter::allow(Arc::clone(&stage_b_calls)),
        rate_policy(10),
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap();
    let endpoint = chigiri::Endpoint::builder(schema)
        .inject(|components| {
            components.rate_limit().stage_b_provider(stage_b);
        })
        .without_context()
        .build()
        .unwrap();

    let response = execute(&endpoint, "{ greeting }");
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(inherited_calls.load(Ordering::SeqCst), 0);
    assert_eq!(stage_b_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn runs_stage_b_before_draft_plugins() {
    let generated_stage = Arc::new(AtomicUsize::new(0));
    let generated_bypass = Arc::new(AtomicUsize::new(0));
    let built_stage = Arc::new(AtomicUsize::new(0));
    let built_bypass = Arc::new(AtomicUsize::new(0));
    let generated = rejecting_endpoint(
        resolvers(),
        Arc::clone(&generated_stage),
        Arc::clone(&generated_bypass),
    );
    let built = rejecting_endpoint(
        schema_builder().build().unwrap(),
        Arc::clone(&built_stage),
        Arc::clone(&built_bypass),
    );

    let generated_response = execute(&generated, "{ greeting }");
    let built_response = execute(&built, "{ greeting }");
    assert_eq!(code(&generated_response), Some("RATE_LIMITED"));
    assert_eq!(generated_response, built_response);
    assert_eq!(generated_stage.load(Ordering::SeqCst), 1);
    assert_eq!(built_stage.load(Ordering::SeqCst), 1);
    assert_eq!(generated_bypass.load(Ordering::SeqCst), 0);
    assert_eq!(built_bypass.load(Ordering::SeqCst), 0);
}

fn rejecting_endpoint(
    input: impl chigiri::IntoEndpointInput<()>,
    stage_calls: Arc<AtomicUsize>,
    bypass_calls: Arc<AtomicUsize>,
) -> chigiri::Endpoint<()> {
    let stage_b = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        CountingLimiter::reject(stage_calls),
        rate_policy(10),
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::new(10).unwrap(),
    )
    .unwrap();
    chigiri::Endpoint::builder(input)
        .inject(|components| {
            components.rate_limit().stage_b_provider(stage_b);
            components
                .plugins()
                .push_plugin(ShortCircuitOperations(bypass_calls));
            components
                .observability()
                .request_id_factory(FixedRequestIds);
        })
        .without_context()
        .build()
        .unwrap()
}

struct CountOperations(Arc<AtomicUsize>);

impl chigiri::Plugin<()> for CountOperations {
    fn operation<'a>(
        &'a self,
        context: chigiri::OperationContext<'a, ()>,
        next: chigiri::OperationNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }
}

struct ShortCircuitOperations(Arc<AtomicUsize>);

impl chigiri::Plugin<()> for ShortCircuitOperations {
    fn operation<'a>(
        &'a self,
        _: chigiri::OperationContext<'a, ()>,
        _: chigiri::OperationNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            chigiri::GraphQlResponse::request_error(
                chigiri::GraphQlError::new("bypassed").code("BYPASSED"),
            )
        })
    }
}

struct FixedRequestIds;

impl chigiri::RequestIdFactory for FixedRequestIds {
    fn create(&self) -> Result<chigiri::RequestId, chigiri::RequestIdFactoryError> {
        chigiri::RequestId::new("request-stage-b").map_err(chigiri::RequestIdFactoryError::new)
    }
}
