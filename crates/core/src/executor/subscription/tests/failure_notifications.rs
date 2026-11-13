use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use crate::executor::{
    ExecutionFailureKind, InternalErrorHandler, Plugin, PreparedSubscriptionOperation,
    PreparedSubscriptionRoot, ResolvedValue, SubscriptionExecutionInput, SubscriptionSetupOutcome,
    execute_subscription_event, prepare_subscription_root,
};
use crate::{
    ExecutionLimits, FieldError, Object, OperationIdentity, PathSegment, RequestTelemetry, Value,
};

use super::fixture::Fixture;
use super::plugin_support::{
    OperationReplaceAfterNext, SessionSetup, StaticEvent, extension_response,
};
use super::support::{Event, EventRow, TestContext, TestDispatch, block_on};

#[test]
fn resource_notifications_cover_node_list_and_custom_scalar_limits() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);

    let mut node_limits = ExecutionLimits::unbounded();
    node_limits.max_output_nodes = 1;
    let operation = fixture.operation(node_limits);
    let root = prepare(&fixture, &dispatch, &operation);
    let event = Event::new("node", "limit");
    assert_resource_limit(&dispatch, &root, &event);

    let mut list_limits = ExecutionLimits::unbounded();
    list_limits.max_list_items_per_field = 1;
    let operation = fixture.operation_for(
        "subscription { items { label } }",
        Object::new(),
        list_limits,
    );
    let root = prepare(&fixture, &dispatch, &operation);
    let event = StaticEvent::new(Ok(ResolvedValue::List(vec![
        Ok(event_object("first")),
        Ok(event_object("second")),
    ])));
    assert_resource_limit(&dispatch, &root, &event);

    let mut scalar_limits = ExecutionLimits::unbounded();
    scalar_limits.max_custom_scalar_output_depth = 0;
    let operation = fixture.operation_for("subscription { payload }", Object::new(), scalar_limits);
    let root = prepare(&fixture, &dispatch, &operation);
    let event = StaticEvent::new(Ok(ResolvedValue::Leaf(Value::List(vec![Value::from(
        "nested",
    )]))));
    assert_resource_limit(&dispatch, &root, &event);
}

#[test]
fn runtime_byte_failure_is_reported_once_and_scoped_to_one_event() {
    let fixture = Fixture::new();
    let setup_dispatch = TestDispatch::new(false);
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let root = prepare(&fixture, &setup_dispatch, &operation);
    let plugins = Vec::new();

    let limited = TestDispatch::new(false).with_byte_limit(69);
    let limited_context = TestContext { generation: 2 };
    let limited_handler = RecordingFailures::default();
    let event = Event::new("", "");
    let response = block_on(execute_subscription_event(
        input(&limited, &plugins, &limited_context, Some(&limited_handler)),
        &root,
        &event,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(
        limited_handler.kinds(),
        [ExecutionFailureKind::ResourceLimit]
    );

    let exact = TestDispatch::new(false).with_byte_limit(70);
    let exact_context = TestContext { generation: 3 };
    let exact_handler = RecordingFailures::default();
    let event = Event::new("", "");
    let response = block_on(execute_subscription_event(
        input(&exact, &plugins, &exact_context, Some(&exact_handler)),
        &root,
        &event,
    ));
    assert!(response.errors.is_empty(), "{response:?}");
    assert!(exact_handler.kinds().is_empty());
}

#[test]
fn typed_failure_survives_response_rewriting_but_ordinary_errors_do_not_signal() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_output_nodes = 1;
    let operation = fixture.operation(limits);
    let root = prepare(&fixture, &dispatch, &operation);
    let plugins: Vec<Arc<dyn Plugin<TestContext>>> =
        vec![Arc::new(OperationReplaceAfterNext(extension_response()))];
    let context = TestContext { generation: 2 };
    let handler = RecordingFailures::default();
    let event = Event::new("rewritten", "response");
    let response = block_on(execute_subscription_event(
        input(&dispatch, &plugins, &context, Some(&handler)),
        &root,
        &event,
    ));
    assert_eq!(
        response.extensions.get("shortCircuit"),
        Some(&Value::from(true))
    );
    assert_resource_kinds(&handler.kinds());

    let operation = fixture.operation(ExecutionLimits::unbounded());
    let root = prepare(&fixture, &dispatch, &operation);
    let plugins = Vec::new();
    let handler = RecordingFailures::default();
    let event = StaticEvent::new(Err(FieldError::internal(OrdinaryFailure)));
    let response = block_on(execute_subscription_event(
        input(&dispatch, &plugins, &context, Some(&handler)),
        &root,
        &event,
    ));
    assert_eq!(response.errors[0].message, "Internal server error");
    assert!(handler.kinds().is_empty());

    let event = StaticEvent::new(Ok(ResolvedValue::Null));
    let _ = block_on(execute_subscription_event(
        input(&dispatch, &plugins, &context, Some(&handler)),
        &root,
        &event,
    ));
    assert!(handler.kinds().is_empty());
}

#[test]
fn prepared_root_identity_violation_reports_an_invariant_before_event_dispatch() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let mut root = prepare(&fixture, &dispatch, &operation);
    root.response_key = "changed".to_owned();
    let plugins = Vec::new();
    let context = TestContext { generation: 2 };
    let handler = RecordingFailures::default();
    let event = Event::new("unused", "unused");

    let response = block_on(execute_subscription_event(
        input(&dispatch, &plugins, &context, Some(&handler)),
        &root,
        &event,
    ));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(handler.kinds(), [ExecutionFailureKind::Invariant]);
    assert_eq!(event.calls.load(Ordering::SeqCst), 0);
}

fn prepare(
    fixture: &Fixture,
    dispatch: &TestDispatch,
    operation: &PreparedSubscriptionOperation,
) -> PreparedSubscriptionRoot {
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let setup = SessionSetup::new("session".to_owned());
    let outcome = block_on(prepare_subscription_root(
        operation,
        fixture.input(dispatch, &plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    *root
}

fn assert_resource_limit(
    dispatch: &TestDispatch,
    root: &PreparedSubscriptionRoot,
    event: &dyn crate::executor::SubscriptionEventDispatch<TestContext>,
) {
    let plugins = Vec::new();
    let context = TestContext { generation: 2 };
    let handler = RecordingFailures::default();
    let response = block_on(execute_subscription_event(
        input(dispatch, &plugins, &context, Some(&handler)),
        root,
        event,
    ));
    assert!(!response.errors.is_empty());
    assert_resource_kinds(&handler.kinds());
}

fn assert_resource_kinds(kinds: &[ExecutionFailureKind]) {
    assert!(!kinds.is_empty());
    assert!(
        kinds
            .iter()
            .all(|kind| *kind == ExecutionFailureKind::ResourceLimit)
    );
}

fn input<'a>(
    dispatch: &'a TestDispatch,
    plugins: &'a [Arc<dyn Plugin<TestContext>>],
    context: &'a TestContext,
    errors: Option<&'a dyn InternalErrorHandler>,
) -> SubscriptionExecutionInput<'a, TestContext> {
    SubscriptionExecutionInput::new(dispatch, plugins, context, errors, None, None, None, None)
}

fn event_object(label: &'static str) -> ResolvedValue {
    ResolvedValue::object(
        "Event",
        EventRow {
            required: "unused",
            label,
        },
    )
}

#[derive(Default)]
struct RecordingFailures(Mutex<Vec<ExecutionFailureKind>>);

impl RecordingFailures {
    fn kinds(&self) -> Vec<ExecutionFailureKind> {
        self.0.lock().unwrap().clone()
    }
}

impl InternalErrorHandler for RecordingFailures {
    fn handle_internal_error(
        &self,
        _error: &mut FieldError,
        _identity: &OperationIdentity,
        _telemetry: Option<&RequestTelemetry>,
        _path: &[PathSegment],
        _coordinate: Option<&str>,
        _component: &'static str,
    ) {
    }

    fn handle_execution_failure(&self, kind: ExecutionFailureKind) {
        self.0.lock().unwrap().push(kind);
    }
}

#[derive(Debug)]
struct OrdinaryFailure;

impl std::fmt::Display for OrdinaryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ordinary application failure")
    }
}

impl std::error::Error for OrdinaryFailure {}
