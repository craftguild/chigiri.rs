use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::executor::{
    Plugin, SubscriptionSetupOutcome, execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, Value};

use super::fixture::Fixture;
use super::plugin_support::{
    DropSession, OperationExtension, OperationReplaceAfterNext, OperationShortCircuit,
    RootFieldErrorAfterNext, RootFieldShortCircuit, SessionSetup, extension_response,
};
use super::support::{Event, Setup, TestContext, TestDispatch, block_on};

#[test]
fn setup_operation_and_root_field_short_circuits_skip_the_source_terminal() {
    let fixture = Fixture::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let dispatch = TestDispatch::new(false);
    let context = TestContext { generation: 1 };

    let setup = Setup::new();
    let operation_plugins: Vec<Arc<dyn Plugin<TestContext>>> =
        vec![Arc::new(OperationShortCircuit(extension_response()))];
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &operation_plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Response(response) = outcome else {
        panic!("operation plugin must short circuit setup")
    };
    assert_eq!(
        response.extensions.get("shortCircuit"),
        Some(&Value::from(true))
    );
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);

    let setup = Setup::new();
    let field_plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![Arc::new(RootFieldShortCircuit)];
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &field_plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Response(response) = outcome else {
        panic!("field plugin must short circuit setup")
    };
    assert_eq!(response.errors[0].message, "root field rejected");
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("ROOT_REJECTED")
    );
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn setup_drops_a_captured_session_after_field_or_operation_post_processing() {
    for plugin in [
        Arc::new(RootFieldErrorAfterNext) as Arc<dyn Plugin<TestContext>>,
        Arc::new(OperationReplaceAfterNext(extension_response())),
        Arc::new(OperationExtension),
    ] {
        let fixture = Fixture::new();
        let operation = fixture.operation(ExecutionLimits::unbounded());
        let dispatch = TestDispatch::new(false);
        let plugins = vec![plugin];
        let context = TestContext { generation: 1 };
        let drops = Arc::new(AtomicUsize::new(0));
        let setup = SessionSetup::new(DropSession::new(&drops));

        let outcome = block_on(prepare_subscription_root(
            &operation,
            fixture.input(&dispatch, &plugins, &context, None),
            &setup,
        ));
        assert!(matches!(outcome, SubscriptionSetupOutcome::Response(_)));
        assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn event_operation_short_circuit_and_post_processing_preserve_response_extensions() {
    let fixture = Fixture::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let dispatch = TestDispatch::new(false);
    let setup_plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let setup = Setup::new();
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &setup_plugins, &setup_context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };

    let short_plugins: Vec<Arc<dyn Plugin<TestContext>>> =
        vec![Arc::new(OperationShortCircuit(extension_response()))];
    let short_event = Event::new("unused", "unused");
    let event_context = TestContext { generation: 2 };
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &short_plugins, &event_context, None),
        &root,
        &short_event,
    ));
    assert_eq!(
        response.extensions.get("shortCircuit"),
        Some(&Value::from(true))
    );
    assert_eq!(short_event.calls.load(Ordering::SeqCst), 0);

    let field_plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![Arc::new(RootFieldShortCircuit)];
    let short_event = Event::new("unused", "unused");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &field_plugins, &event_context, None),
        &root,
        &short_event,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors[0].message, "root field rejected");
    assert_eq!(short_event.calls.load(Ordering::SeqCst), 0);

    let field_plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![Arc::new(RootFieldErrorAfterNext)];
    let post_processed = Event::new("unused", "unused");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &field_plugins, &event_context, None),
        &root,
        &post_processed,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(
        response.errors[0].message,
        "root field post-processing failed"
    );
    assert_eq!(post_processed.calls.load(Ordering::SeqCst), 1);

    let extension_plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![Arc::new(OperationExtension)];
    let event = Event::new("value", "post-processed");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &extension_plugins, &event_context, None),
        &root,
        &event,
    ));
    assert!(response.errors.is_empty(), "{response:?}");
    assert_eq!(
        response.extensions.get("operationPlugin"),
        Some(&Value::from(true))
    );
    assert_eq!(event.calls.load(Ordering::SeqCst), 1);
}
