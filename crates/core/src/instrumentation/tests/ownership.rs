//! Actual poll, thread movement, unwind and destruction under the owning scope.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::support::{CURRENT, Guard, scope};
use crate::instrumentation::InstrumentationKind;
use crate::{LifecycleResult, SamplingDecision};

type Calls = Arc<Mutex<Vec<(&'static str, (u8, bool))>>>;

struct Probe {
    pending: bool,
    panic_drop: bool,
    calls: Calls,
}

impl Future for Probe {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        self.calls.lock().unwrap().push(("poll", CURRENT.get()));
        if self.pending {
            self.pending = false;
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.calls.lock().unwrap().push(("drop", CURRENT.get()));
        assert!(!self.panic_drop, "test application destruction failed");
    }
}

#[test]
fn moved_future_restores_both_ambient_states_and_disposes_under_its_owner() {
    for suppressed in [false, true] {
        let _outer = Guard::install((9, !suppressed));
        let (scope, ends) = scope((1, suppressed));
        let calls = Calls::default();
        let future = scope.instrument(Probe {
            pending: true,
            panic_drop: false,
            calls: calls.clone(),
        });
        let mut future = std::thread::spawn(move || {
            let _outer = Guard::install((8, !suppressed));
            let mut future = Box::pin(future);
            let mut context = Context::from_waker(Waker::noop());
            assert!(future.as_mut().poll(&mut context).is_pending());
            assert_eq!(CURRENT.get(), (8, !suppressed));
            future
        })
        .join()
        .unwrap();
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_ready());
        assert_eq!(CURRENT.get(), (9, !suppressed));
        drop(future);
        assert_eq!(CURRENT.get(), (9, !suppressed));
        assert_eq!(
            *calls.lock().unwrap(),
            [
                ("poll", (1, suppressed)),
                ("poll", (1, suppressed)),
                ("drop", (1, suppressed))
            ]
        );
        scope.finish(LifecycleResult::TimedOut);
        scope.clone().finish(LifecycleResult::Succeeded);
        drop(scope);
        assert_eq!(
            *ends.lock().unwrap(),
            [(LifecycleResult::TimedOut, (1, suppressed))]
        );
        assert_eq!(CURRENT.get(), (9, !suppressed));
    }
}

#[test]
fn poll_unwind_and_failed_destruction_restore_the_original_ambient_context() {
    let _outer = Guard::install((9, true));
    let (scope, ends) = scope((1, false));
    let mut future = Box::pin(scope.instrument(async {
        assert_eq!(CURRENT.get(), (1, false));
        panic!("test application poll failed");
    }));
    let mut context = Context::from_waker(Waker::noop());
    assert!(catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut context))).is_err());
    assert_eq!(CURRENT.get(), (9, true));
    drop(future);
    let calls = Calls::default();
    let future = scope.instrument(Probe {
        pending: true,
        panic_drop: true,
        calls: calls.clone(),
    });
    assert!(catch_unwind(AssertUnwindSafe(|| drop(future))).is_err());
    assert_eq!(*calls.lock().unwrap(), [("drop", (1, false))]);
    assert_eq!(CURRENT.get(), (9, true));
    drop(scope);
    assert_eq!(
        *ends.lock().unwrap(),
        [(LifecycleResult::Cancelled, (1, false))]
    );
}

#[test]
fn child_capture_preserves_nested_owning_suppression_without_leaking_it() {
    let _outer = Guard::install((9, false));
    let (scope, _) = scope((1, false));
    scope.run(|| {
        let _nested = Guard::install((1, true));
        let child = scope.child(InstrumentationKind::Field, SamplingDecision::Record);
        assert!(child.context().is_suppressed());
        child.run(|| assert_eq!(CURRENT.get(), (2, true)));
        assert_eq!(CURRENT.get(), (1, true));
    });
    assert_eq!(CURRENT.get(), (9, false));
    assert!(!scope.context().is_suppressed());
}

#[test]
fn retained_telemetry_keeps_suppression_without_holding_a_live_scope_or_request_id() {
    let (scope, ends) = scope((1, true));
    let telemetry = crate::RequestTelemetry::default().with_instrumentation_scope(scope);
    assert!(telemetry.request_id().is_none());
    assert!(telemetry.instrumentation_scope().is_some());
    let retained = telemetry.clone().retain_instrumentation(0);
    assert!(retained.instrumentation_scope().is_none());
    assert!(retained.propagation_context().unwrap().is_suppressed());
    assert_eq!(telemetry, retained);
    assert!(ends.lock().unwrap().is_empty());
    drop(telemetry);
    assert_eq!(
        *ends.lock().unwrap(),
        [(LifecycleResult::Cancelled, (1, true))]
    );
    assert!(retained.propagation_context().unwrap().is_suppressed());
}

#[test]
fn concurrent_completion_has_one_backend_call_and_final_drop_does_not_repeat_it() {
    let (scope, ends) = scope((1, true));
    let first = scope.clone();
    let second = scope.clone();
    let first = std::thread::spawn(move || first.finish(LifecycleResult::Failed));
    let second = std::thread::spawn(move || second.finish(LifecycleResult::Succeeded));
    first.join().unwrap();
    second.join().unwrap();
    drop(scope);
    let ends = ends.lock().unwrap();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].1, (1, true));
}
