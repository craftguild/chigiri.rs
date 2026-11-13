//! Required late host metadata wins over optional propagation at exact capacity.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::SubscriptionObservation;
use chigiri_runtime::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, RequestTelemetry, SamplingDecision};

use super::super::test_support::{self, host};
use super::super::*;
use crate::subscription::worker::observation::{self, HostObservationHandoff};

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
        InstrumentationScope::new(Selected(self.0.clone()))
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

#[derive(Default)]
struct Handoff(Mutex<Option<SubscriptionObservation>>);
impl HostObservationHandoff for Handoff {
    fn transfer(
        &self,
        observation: SubscriptionObservation,
    ) -> Result<(), SubscriptionObservation> {
        assert!(self.0.lock().unwrap().replace(observation).is_none());
        Ok(())
    }
}

#[test]
fn exact_required_host_capacity_discards_optional_correlation_without_rejecting_work() {
    for suppressed in [false, true] {
        let baseline_probe = Arc::new(host::Probe::default());
        let baseline_schema = test_support::with_host(Arc::clone(&baseline_probe));
        let mut baseline = baseline_schema
            .prepare_subscription(test_support::graph_request())
            .unwrap();
        let host_bytes = baseline
            .prepare_host_observation(&(), 4096)
            .unwrap()
            .unwrap()
            .retained_metadata_bytes();
        let probe = Arc::new(host::Probe::default());
        let schema = test_support::with_host(Arc::clone(&probe));
        let telemetry = RequestTelemetry::new(None, None, SamplingDecision::Record)
            .with_instrumentation_scope(InstrumentationScope::new(Selected(
                PropagationContext::new(
                    Some(TraceParent::new([1; 16], [2; 8], 1, false, "vendor=value").unwrap()),
                    suppressed,
                ),
            )));
        let mut prepared = schema
            .prepare_subscription(test_support::graph_request().with_request_telemetry(telemetry))
            .unwrap();
        let metadata = ApplicationMetadata::default();
        let transport = TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
        let ingress = SubscriberIngress::capture(
            transport,
            &prepared.retained_request(),
            RequestMethod::Post,
            ResponseMediaType::Json,
            SubscriptionTransport::ServerSentEvents,
            None,
            MetadataRetentionLimit::default(),
        )
        .unwrap();
        let required = prepared.required_retained_metadata_bytes().unwrap() + ingress.charged_bytes;
        let limit = required + host_bytes;
        let mut request = SubscriberRequest::from_ingress(
            ingress,
            &prepared,
            MetadataRetentionLimit::new(limit).unwrap(),
        )
        .unwrap();
        assert!(request.charged_bytes() > required);
        let pointer = Arc::as_ptr(&prepared.retained_request());
        let query = request.request().query.as_ptr();
        let handoff = Handoff::default();
        let result = observation::transfer(&mut prepared, &(), &mut request, limit, &handoff);
        assert!(
            result.is_ok(),
            "required host bytes must displace optional tracing: {result:?}"
        );
        assert_eq!(request.charged_bytes(), limit);
        assert_eq!(Arc::as_ptr(&prepared.retained_request()), pointer);
        assert_eq!(request.request().query.as_ptr(), query);
        let retained = request.request().request_telemetry().unwrap();
        assert!(retained.instrumentation_scope().is_none());
        assert!(retained.trace_id().is_none());
        let values = retained.propagation_context().unwrap();
        assert!(values.parent().is_none());
        assert_eq!(values.is_suppressed(), suppressed);
        assert_eq!(probe.budgets.lock().unwrap().as_slice(), [host::CHARGE]);
        assert_eq!(probe.allocated.load(Ordering::SeqCst), 1);
        assert_eq!(probe.dropped.load(Ordering::SeqCst), 0);
        let mut owner = handoff.0.lock().unwrap().take().unwrap();
        assert!(owner.accept());
        drop(owner);
        assert_eq!(probe.dropped.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn absent_host_restores_correlation_without_copying_the_request() {
    for suppressed in [false, true] {
        let telemetry = RequestTelemetry::new(None, None, SamplingDecision::Drop)
            .with_instrumentation_scope(InstrumentationScope::new(Selected(
                PropagationContext::new(
                    Some(TraceParent::new([3; 16], [4; 8], 0, true, "vendor=value").unwrap()),
                    suppressed,
                ),
            )));
        let mut prepared =
            test_support::prepared(test_support::graph_request().with_request_telemetry(telemetry));
        let metadata = ApplicationMetadata::default();
        let transport = TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
        let ingress = SubscriberIngress::capture(
            transport,
            &prepared.retained_request(),
            RequestMethod::Post,
            ResponseMediaType::Json,
            SubscriptionTransport::ServerSentEvents,
            None,
            MetadataRetentionLimit::default(),
        )
        .unwrap();
        let limit = prepared.retained_metadata_bytes().unwrap() + ingress.charged_bytes;
        let mut request = SubscriberRequest::from_ingress(
            ingress,
            &prepared,
            MetadataRetentionLimit::new(limit).unwrap(),
        )
        .unwrap();
        let before = request.request().request_telemetry().unwrap().clone();
        let pointer = Arc::as_ptr(&prepared.retained_request());
        let handoff = Handoff::default();
        observation::transfer(&mut prepared, &(), &mut request, limit, &handoff).unwrap();
        assert_eq!(request.charged_bytes(), limit);
        assert_eq!(Arc::as_ptr(&prepared.retained_request()), pointer);
        let after = request.request().request_telemetry().unwrap();
        assert_eq!(after, &before);
        assert_eq!(after.propagation_context(), before.propagation_context());
        assert_eq!(after.sampling(), SamplingDecision::Drop);
        assert!(handoff.0.lock().unwrap().is_none());
    }
}

#[test]
fn oversized_required_host_data_rejects_before_payload_allocation() {
    let baseline = test_support::with_host(Arc::new(host::Probe::default()));
    let mut baseline = baseline
        .prepare_subscription(test_support::graph_request())
        .unwrap();
    let host_bytes = baseline
        .prepare_host_observation(&(), 4096)
        .unwrap()
        .unwrap()
        .retained_metadata_bytes();
    let probe = Arc::new(host::Probe::default());
    let schema = test_support::with_host(Arc::clone(&probe));
    let mut prepared = schema
        .prepare_subscription(test_support::graph_request())
        .unwrap();
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
    let ingress = SubscriberIngress::capture(
        transport,
        &prepared.retained_request(),
        RequestMethod::Post,
        ResponseMediaType::Json,
        SubscriptionTransport::ServerSentEvents,
        None,
        MetadataRetentionLimit::default(),
    )
    .unwrap();
    let mut request =
        SubscriberRequest::from_ingress(ingress, &prepared, MetadataRetentionLimit::default())
            .unwrap();
    let handoff = Handoff::default();
    let limit = request.charged_bytes() + host_bytes - 1;
    let result = observation::transfer(&mut prepared, &(), &mut request, limit, &handoff);
    assert!(result.is_err());
    assert_eq!(probe.budgets.lock().unwrap().as_slice(), [host::CHARGE - 1]);
    assert_eq!(probe.allocated.load(Ordering::SeqCst), 0);
    assert!(handoff.0.lock().unwrap().is_none());
}
