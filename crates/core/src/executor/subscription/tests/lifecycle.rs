use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::executor::{
    Plugin, SubscriptionSetupOutcome, execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, LifecyclePhase, LifecycleResult};

use super::fixture::Fixture;
use super::plugin_support::{OperationShortCircuit, extension_response};
use super::support::{Event, LifecycleLog, Setup, TestContext, TestDispatch, block_on};

#[test]
fn setup_and_event_success_each_report_a_complete_lifecycle() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let setup_lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let setup = Setup::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &setup_context, Some(&setup_lifecycle)),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    assert_eq!(*setup_lifecycle.0.lock().unwrap(), successful_lifecycle());

    let event_lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let context = TestContext { generation: 2 };
    let event = Event::new("required", "label");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, Some(&event_lifecycle)),
        &root,
        &event,
    ));
    assert!(response.errors.is_empty());
    assert_eq!(*event_lifecycle.0.lock().unwrap(), successful_lifecycle());
}

#[test]
fn operation_short_circuits_reject_setup_and_event_before_the_terminal() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let context = TestContext { generation: 1 };
    let rejecting: Vec<Arc<dyn Plugin<TestContext>>> =
        vec![Arc::new(OperationShortCircuit(extension_response()))];
    let lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let setup = Setup::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &rejecting, &context, Some(&lifecycle)),
        &setup,
    ));
    assert!(matches!(outcome, SubscriptionSetupOutcome::Response(_)));
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);
    assert_eq!(*lifecycle.0.lock().unwrap(), rejected_lifecycle());

    let plugins = Vec::new();
    let setup = Setup::new();
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    let lifecycle = LifecycleLog(std::sync::Mutex::new(Vec::new()));
    let event = Event::new("unused", "unused");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &rejecting, &context, Some(&lifecycle)),
        &root,
        &event,
    ));
    assert_eq!(response.extensions.get("shortCircuit"), Some(&true.into()));
    assert_eq!(event.calls.load(Ordering::SeqCst), 0);
    assert_eq!(*lifecycle.0.lock().unwrap(), rejected_lifecycle());
}

fn successful_lifecycle() -> [(LifecyclePhase, LifecycleResult); 4] {
    [
        (LifecyclePhase::OperationAdmission, LifecycleResult::Started),
        (
            LifecyclePhase::OperationAdmission,
            LifecycleResult::Succeeded,
        ),
        (LifecyclePhase::Execution, LifecycleResult::Started),
        (LifecyclePhase::Execution, LifecycleResult::Succeeded),
    ]
}

fn rejected_lifecycle() -> [(LifecyclePhase, LifecycleResult); 2] {
    [
        (LifecyclePhase::OperationAdmission, LifecycleResult::Started),
        (
            LifecyclePhase::OperationAdmission,
            LifecycleResult::Rejected,
        ),
    ]
}
