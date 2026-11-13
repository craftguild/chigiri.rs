//! A retained supervision reporter cannot keep a finished setup owner alive.

mod activation;
mod retention;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_runtime::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext, TraceParent,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, RequestTelemetry, SamplingDecision};

use super::SubscriptionFailureReporter;

/// Exact retention charges one allocation per immutable identity text.
pub(crate) fn assert_shared_ids(
    expected: Option<&RequestTelemetry>,
    actual: Option<&RequestTelemetry>,
) {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        assert_eq!(expected.is_some(), actual.is_some());
        return;
    };
    assert_eq!(expected, actual);
    assert_eq!(
        expected.request_id().map(|id| id.as_str().as_ptr()),
        actual.request_id().map(|id| id.as_str().as_ptr())
    );
    assert_eq!(
        expected.trace_id().map(|id| id.as_str().as_ptr()),
        actual.trace_id().map(|id| id.as_str().as_ptr())
    );
}

struct Setup(Arc<AtomicUsize>, PropagationContext);
struct Guard;
impl InstrumentationGuard for Guard {}
impl Drop for Setup {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
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
        unreachable!("supervision must not create setup children")
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

#[test]
fn retained_supervision_releases_the_finished_setup_scope_before_source_waiting() {
    for suppressed in [false, true] {
        let dropped = Arc::new(AtomicUsize::new(0));
        let scope = InstrumentationScope::new(Setup(
            Arc::clone(&dropped),
            PropagationContext::new(
                Some(TraceParent::new([1; 16], [2; 8], 1, true, "vendor=value").unwrap()),
                suppressed,
            ),
        ));
        let telemetry = RequestTelemetry::new(None, None, SamplingDecision::Record)
            .with_instrumentation_scope(scope.clone());
        let reporter = SubscriptionFailureReporter::new(
            crate::subscription::request::test_support::schema(),
            telemetry,
            "supervision-fixture",
        );
        let retained = reporter.clone();
        scope.finish(LifecycleResult::Succeeded);
        drop(scope);
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "reporter clones retain values, never the setup owner"
        );
        drop(reporter);
        drop(retained);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}
