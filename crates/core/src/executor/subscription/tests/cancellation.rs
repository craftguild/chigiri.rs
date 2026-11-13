use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use crate::executor::{
    BoxFuture, FieldContext, Plugin, SubscriptionEventDispatch, SubscriptionSetupOutcome,
    execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, FieldResult, LifecyclePhase, LifecycleResult};

use super::fixture::Fixture;
use super::plugin_support::{DropSession, PendingAfterNext, SessionSetup};
use super::support::{LifecycleLog, PendingEvent, Setup, TestContext, TestDispatch, block_on};

struct NeverEvent {
    dropped: Arc<AtomicBool>,
}

#[test]
fn dropping_setup_after_session_capture_drops_the_session_and_cancels_execution() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let setup = SessionSetup::new(DropSession::new(&drops));
    let plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![Arc::new(PendingAfterNext)];
    let context = TestContext { generation: 1 };
    let lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let mut future = prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &context, Some(&lifecycle)),
        &setup,
    );
    let mut task_context = Context::from_waker(Waker::noop());

    assert!(matches!(
        future.as_mut().poll(&mut task_context),
        Poll::Pending
    ));
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(future);

    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        *lifecycle.0.lock().unwrap(),
        [
            (LifecyclePhase::OperationAdmission, LifecycleResult::Started),
            (
                LifecyclePhase::OperationAdmission,
                LifecycleResult::Succeeded
            ),
            (LifecyclePhase::Execution, LifecycleResult::Started),
            (LifecyclePhase::Execution, LifecycleResult::Cancelled),
        ]
    );
}

impl SubscriptionEventDispatch<TestContext> for NeverEvent {
    fn resolve_event<'a>(
        &'a self,
        _context: FieldContext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<crate::executor::ResolvedValue>> {
        Box::pin(PendingEvent {
            dropped: Arc::clone(&self.dropped),
        })
    }
}

#[test]
fn dropping_a_pending_event_cancels_execution_and_drops_the_terminal_future() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let setup = Setup::new();
    let plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &setup_context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    let lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let dropped = Arc::new(AtomicBool::new(false));
    let event = NeverEvent {
        dropped: Arc::clone(&dropped),
    };
    let event_context = TestContext { generation: 2 };
    let mut future = execute_subscription_event(
        fixture.input(&dispatch, &plugins, &event_context, Some(&lifecycle)),
        &root,
        &event,
    );
    let mut task_context = Context::from_waker(Waker::noop());

    assert!(matches!(
        future.as_mut().poll(&mut task_context),
        Poll::Pending
    ));
    assert!(!dropped.load(Ordering::SeqCst));
    drop(future);

    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(
        *lifecycle.0.lock().unwrap(),
        [
            (LifecyclePhase::OperationAdmission, LifecycleResult::Started),
            (
                LifecyclePhase::OperationAdmission,
                LifecycleResult::Succeeded
            ),
            (LifecyclePhase::Execution, LifecycleResult::Started),
            (LifecyclePhase::Execution, LifecycleResult::Cancelled),
        ]
    );
}
