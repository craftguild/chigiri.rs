//! Optional finite trace-ID text cannot displace otherwise admissible metadata.

use chigiri_runtime::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, RequestTelemetry, SamplingDecision};

use super::super::*;

struct Selected(PropagationContext);
struct Guard;
impl InstrumentationGuard for Guard {}
impl ExecutionScope for Selected {
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard)
    }
    fn context(&self) -> PropagationContext {
        self.0.clone()
    }
    fn child(&self, _: InstrumentationKind, _: SamplingDecision) -> InstrumentationScope {
        InstrumentationScope::new(Self(self.0.clone()))
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

#[test]
fn finite_optional_id_degrades_at_required_capacity_without_losing_execution_scope() {
    for suppressed in [false, true] {
        for available in [0, 31, 32] {
            let values = PropagationContext::new(
                Some(TraceParent::new([1; 16], [2; 8], 1, true, "vendor=value").unwrap()),
                suppressed,
            );
            let telemetry = RequestTelemetry::new(None, None, SamplingDecision::Record)
                .with_instrumentation_scope(InstrumentationScope::new(Selected(values.clone())));
            let request = GraphQlRequest::new("{ ready }").with_request_telemetry(telemetry);
            let metadata = ApplicationMetadata::default();
            let transport = TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
            let ingress = SubscriberIngress::capture_websocket(
                transport,
                &request,
                RequestMethod::Post,
                ResponseMediaType::GraphQlResponseJson,
                None,
                MetadataRetentionLimit::default(),
            )
            .unwrap();
            let mut prepared = test_support::schema()
                .prepare_websocket_operation(request)
                .unwrap();
            let bytes = prepared.retained_metadata_bytes().unwrap() + ingress.charged_bytes;
            let required =
                prepared.required_retained_metadata_bytes().unwrap() + ingress.charged_bytes;
            assert_eq!(bytes - required, 32);
            let pointer = Arc::as_ptr(&prepared.retained_request());
            let query = prepared.retained_request().query.as_ptr();
            let limit = MetadataRetentionLimit::new(required + available).unwrap();
            SubscriberRequest::budget_websocket_instrumentation(&ingress, &mut prepared, limit)
                .unwrap();
            let retained = SubscriberRequest::from_websocket(ingress, &prepared, limit).unwrap();
            assert_eq!(
                retained.charged_bytes(),
                required + usize::from(available >= 32) * 32
            );
            assert_eq!(Arc::as_ptr(&prepared.retained_request()), pointer);
            assert_eq!(retained.request().query.as_ptr(), query);
            let telemetry = retained.request().request_telemetry().unwrap();
            assert!(telemetry.instrumentation_scope().is_some());
            assert_eq!(telemetry.propagation_context(), Some(values));
            assert_eq!(telemetry.trace_id().is_some(), available >= 32);
            assert_eq!(telemetry.sampling(), SamplingDecision::Record);
        }
    }
}
