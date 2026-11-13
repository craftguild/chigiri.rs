//! Supervision activates the final budgeted snapshot without worker fallback.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use chigiri_runtime::instrumentation::{
    ExecutionScope, Instrumentation, InstrumentationGuard, InstrumentationKind,
    InstrumentationScope, PropagationCarrier, PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, RequestTelemetry, SamplingDecision};

use super::super::{SubscriptionFailureClass, SubscriptionFailureReporter};

thread_local! {
    static CURRENT: RefCell<PropagationContext> = const { RefCell::new(PropagationContext::new(None, false)) };
}

struct Values(PropagationContext);
struct Guard(Option<PropagationContext>);
impl InstrumentationGuard for Guard {}
impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0.take().unwrap());
    }
}
impl ExecutionScope for Values {
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard(Some(
            CURRENT.with(|slot| slot.replace(self.0.clone())),
        )))
    }
    fn context(&self) -> PropagationContext {
        self.0.clone()
    }
    fn child(&self, _: InstrumentationKind, _: SamplingDecision) -> InstrumentationScope {
        unreachable!("supervision has no lifecycle children")
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

struct Provider;
impl Instrumentation for Provider {
    fn capture(&self, _: &dyn PropagationCarrier) -> PropagationContext {
        unreachable!("supervision must use its explicit retained snapshot")
    }
    fn start(
        &self,
        values: &PropagationContext,
        _: InstrumentationKind,
        sampling: SamplingDecision,
    ) -> InstrumentationScope {
        assert_eq!(sampling, SamplingDecision::Drop);
        InstrumentationScope::new(Values(values.clone()))
    }
}

#[derive(Clone)]
struct Observer(Arc<Mutex<Vec<PropagationContext>>>);
impl chigiri_runtime::ErrorObserver for Observer {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(CURRENT.with(|slot| slot.borrow().clone()));
    }
}

#[test]
fn final_supervision_snapshot_survives_zero_capacity_and_restores_both_worker_controls() {
    for suppressed in [false, true] {
        for retained in [false, true] {
            for sampling in [SamplingDecision::Record, SamplingDecision::Drop] {
                let values = PropagationContext::new(
                    Some(TraceParent::new([1; 16], [2; 8], 1, true, "vendor=value").unwrap()),
                    suppressed,
                );
                let telemetry = RequestTelemetry::new(None, None, sampling)
                    .with_instrumentation_scope(InstrumentationScope::new(Values(values)));
                let detached =
                    telemetry.retained_instrumentation(if retained { usize::MAX } else { 0 });
                let expected = detached.propagation_context().unwrap();
                let observer = Observer(Arc::default());
                let schema = crate::subscription::request::test_support::builder()
                    .instrumentation(Provider)
                    .error_observer(observer.clone())
                    .build()
                    .unwrap();
                let mut reporter =
                    SubscriptionFailureReporter::new(schema, telemetry, "supervision-fixture");
                assert!(reporter.telemetry.instrumentation_scope().is_none());
                let initial = reporter.telemetry.propagation_context().unwrap();
                assert!(initial.parent().is_none());
                assert_eq!(initial.is_suppressed(), suppressed);
                reporter.bind_retained_telemetry(Some(&detached));
                assert_eq!(
                    reporter.telemetry.propagation_context(),
                    Some(expected.clone())
                );
                let worker = PropagationContext::new(
                    Some(TraceParent::new([9; 16], [9; 8], 1, false, "worker=value").unwrap()),
                    !suppressed,
                );
                let scope = InstrumentationScope::new(Values(worker.clone()));
                scope.run(|| {
                    let response = reporter.response(
                        SubscriptionFailureClass::Internal,
                        &std::io::Error::other("private supervision source"),
                    );
                    assert_eq!(response.errors[0].message, "Internal server error");
                    assert_eq!(CURRENT.with(|slot| slot.borrow().clone()), worker);
                });
                assert_eq!(*observer.0.lock().unwrap(), [expected]);
                assert!(reporter.telemetry.instrumentation_scope().is_none());
            }
        }
    }
}
