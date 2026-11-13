//! Actual worker handoffs exercise retained text-allocation assertions at each copy.

use std::sync::Arc;

use chigiri_runtime::instrumentation::{
    ExecutionScope, Instrumentation, InstrumentationGuard, InstrumentationKind,
    InstrumentationScope, PropagationCarrier, PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, SamplingDecision};

use super::state::{SourceStep, State, count};
use super::{harness, http_worker::support as http, websocket_worker::support as websocket};
use crate::subscription::worker::websocket::{
    WebSocketSubscriptionSetupOutcome, WebSocketValidationOutcome,
};
use crate::transport::streaming::PreparedHttpSubscription;

struct Provider(PropagationContext);
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
impl Instrumentation for Provider {
    fn capture(&self, _: &dyn PropagationCarrier) -> PropagationContext {
        self.0.clone()
    }
    fn start(
        &self,
        values: &PropagationContext,
        _: InstrumentationKind,
        _: SamplingDecision,
    ) -> InstrumentationScope {
        InstrumentationScope::new(Selected(values.clone()))
    }
}

fn instrument(
    endpoint: &mut Arc<crate::transport::EndpointState<super::state::AppContext>>,
    probe: &Arc<State>,
    suppressed: bool,
) {
    let state = Arc::get_mut(endpoint).unwrap();
    state.schema = state
        .schema
        .reconfigure()
        .request_id_factory(harness::RequestIds(Arc::clone(probe)))
        .instrumentation(Provider(PropagationContext::new(
            Some(TraceParent::new([1; 16], [2; 8], 1, false, "vendor=value").unwrap()),
            suppressed,
        )))
        .build();
    Arc::get_mut(&mut state.subscription_execution)
        .unwrap()
        .schema = state.schema.clone();
}

#[test]
fn actual_http_ready_active_and_supervision_roles_share_retained_ids() {
    let runtime = http::runtime();
    for profile in http::profiles() {
        for suppressed in [false, true] {
            let probe = State::new(vec![SourceStep::Pending]);
            let (mut endpoint, _) = http::endpoint(Arc::clone(&probe));
            instrument(&mut endpoint, &probe, suppressed);
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let pending = http::run(
                &runtime,
                http::pending(endpoint, profile, http::VALID, false),
            );
            // The actual producer compares the retained request with Ready;
            // the active slot feeds the real bound reporter identity assertion.
            let ready = http::run(&runtime, pending.prepare());
            assert!(matches!(&ready, PreparedHttpSubscription::Ready(_)));
            assert_eq!(count(&probe.counts.factory_calls), 1);
            assert_eq!(count(&probe.counts.source_polls), 0);
            drop(ready);
            http::drain(&runtime, guard);
            assert_eq!(count(&probe.counts.source_drops), 1);
            assert_eq!(count(&probe.counts.completion_calls), 1);
        }
    }
}

#[test]
fn actual_websocket_ready_and_bound_reporter_roles_share_retained_ids() {
    for suppressed in [false, true] {
        let probe = State::new(vec![SourceStep::Pending]);
        let mut registered = websocket::Registered::new(Arc::clone(&probe));
        instrument(&mut registered.endpoint, &probe, suppressed);
        let schema = &registered.endpoint.schema;
        let captured = schema.capture_instrumentation(&()).unwrap();
        let telemetry = schema.request_telemetry_with_capture(
            None,
            Some(&captured),
            InstrumentationKind::Request,
        );
        assert!(telemetry.request_id().is_some());
        assert_eq!(telemetry.trace_id().unwrap().as_str().len(), 32);
        let id: Arc<str> = Arc::from("retained-id");
        let validation = websocket::worker(
            &registered.runtime,
            &registered.endpoint,
            harness::request().with_request_telemetry(telemetry),
            None,
            registered.begin(id),
            &websocket::NEVER_DEADLINES,
        );
        let WebSocketValidationOutcome::Prepared(prepared) =
            websocket::run(&registered.runtime, validation)
        else {
            panic!("subscription validation must prepare");
        };
        let setup = {
            let _entered = registered.runtime.enter();
            prepared.activate_subscription()
        }
        .unwrap_or_else(|failure| panic!("subscription activation failed: {:?}", failure.error()));
        let WebSocketSubscriptionSetupOutcome::Ready { subscription, .. } =
            websocket::run(&registered.runtime, setup)
        else {
            panic!("subscription setup must finish");
        };
        assert_eq!(count(&probe.counts.factory_calls), 1);
        assert_eq!(count(&probe.counts.source_polls), 0);
        drop(subscription);
        registered.drain();
        assert_eq!(count(&probe.counts.source_drops), 1);
    }
}
