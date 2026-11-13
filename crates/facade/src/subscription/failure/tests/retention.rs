//! Capacity boundaries retain one actual ID allocation through supervision clones.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_http::{
    ApplicationMetadata, RequestMethod, ResponseMediaType, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::{RequestId, SamplingDecision};

use super::super::SubscriptionFailureReporter;
use super::{Guard, assert_shared_ids};
use crate::subscription::request::{
    MetadataRetentionLimit, SubscriberIngress, SubscriberRequest, test_support,
};
use chigiri_runtime::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, RequestTelemetry};

struct Setup(Option<Arc<AtomicUsize>>, PropagationContext);
impl Drop for Setup {
    fn drop(&mut self) {
        if let Some(dropped) = &self.0 {
            dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl ExecutionScope for Setup {
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard)
    }
    fn context(&self) -> PropagationContext {
        self.1.clone()
    }
    fn child(&self, _: InstrumentationKind, _: SamplingDecision) -> InstrumentationScope {
        InstrumentationScope::new(Setup(None, self.1.clone()))
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

#[test]
fn exact_capacity_supervision_clones_share_charged_text_and_release_setup_owner() {
    for profile in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        for suppressed in [false, true] {
            for sampling in [SamplingDecision::Record, SamplingDecision::Drop] {
                for capacity in [0, 31, 32, 32 + parent_bytes() - 1, 32 + parent_bytes()] {
                    run_case(profile, suppressed, sampling, capacity);
                }
            }
        }
    }
}

fn parent_bytes() -> usize {
    values(false).correlation_bytes()
}
fn values(suppressed: bool) -> PropagationContext {
    PropagationContext::new(
        Some(TraceParent::new([1; 16], [2; 8], 1, true, "vendor=value").unwrap()),
        suppressed,
    )
}

fn run_case(
    profile: SubscriptionTransport,
    suppressed: bool,
    sampling: SamplingDecision,
    capacity: usize,
) {
    let dropped = Arc::new(AtomicUsize::new(0));
    let scope = InstrumentationScope::new(Setup(Some(Arc::clone(&dropped)), values(suppressed)));
    let telemetry = RequestTelemetry::new(
        Some(RequestId::new("required-identity").unwrap()),
        None,
        sampling,
    )
    .with_instrumentation_scope(scope.clone());
    let schema = test_support::schema();
    let mut prepared = schema
        .prepare_subscription(test_support::graph_request().with_request_telemetry(telemetry))
        .unwrap();
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
    let ingress = SubscriberIngress::capture(
        transport,
        &prepared.retained_request(),
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        profile,
        None,
        MetadataRetentionLimit::default(),
    )
    .unwrap();
    let mut source =
        SubscriberRequest::from_ingress(ingress, &prepared, MetadataRetentionLimit::default())
            .unwrap();
    let required = source.charged_bytes() - 32 - parent_bytes();
    let host_bytes = 16;
    let limit = required + host_bytes + capacity;
    assert!(source.reserve_host_metadata(&mut prepared, limit).unwrap() >= host_bytes);
    source
        .charge_host_observation(&mut prepared, host_bytes, limit)
        .unwrap();
    let original = source.request().request_telemetry().unwrap();
    assert!(original.instrumentation_scope().is_none());
    assert_eq!(original.sampling(), sampling);
    assert_eq!(original.trace_id().is_some(), capacity >= 32);
    let context = original.propagation_context().unwrap();
    assert_eq!(context.is_suppressed(), suppressed);
    assert_eq!(context.parent().is_some(), capacity == 32 + parent_bytes());
    let optional =
        original.trace_id().map_or(0, |id| id.as_str().len()) + context.correlation_bytes();
    assert_eq!(source.charged_bytes(), required + host_bytes + optional);
    assert!(source.charged_bytes() <= limit);
    let ready = original.clone();
    assert_shared_ids(Some(original), Some(&ready));
    let mut reporter = SubscriptionFailureReporter::new(
        schema,
        prepared.setup_telemetry().unwrap().clone(),
        "retention-fixture",
    );
    reporter.bind_retained_telemetry(Some(&ready));
    let clones = [reporter.clone(), reporter.clone()];
    for retained in std::iter::once(&reporter).chain(clones.iter()) {
        assert_shared_ids(Some(original), Some(&retained.telemetry));
        assert!(retained.telemetry.instrumentation_scope().is_none());
        assert_eq!(
            retained.telemetry.propagation_context(),
            Some(context.clone())
        );
    }
    scope.finish(LifecycleResult::Succeeded);
    drop(prepared);
    drop(scope);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    for retained in clones {
        assert_shared_ids(Some(original), Some(&retained.telemetry));
    }
    assert_eq!(source.charged_bytes(), required + host_bytes + optional);
}
