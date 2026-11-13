use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::executable::{CoercedVariables, validate_executable_document};
use crate::executor::{
    Plugin, SubscriptionSetupOutcome, execute_operation, execute_subscription_event,
    prepare_subscription_root,
};
use crate::language::parse_executable_document;
use crate::{ExecutionLimits, Location, PathSegment, Source, SourceId, Value};

use super::fixture::Fixture;
use super::support::{Event, RecordingPlugin, Setup, TestContext, TestDispatch, block_on};

#[test]
fn setup_once_and_fresh_events_use_plugins_without_root_redispatch() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let setup = Setup::new();
    let recorder = Arc::new(RecordingPlugin::new());
    let plugins: Vec<Arc<dyn Plugin<TestContext>>> = vec![recorder.clone()];
    let setup_context = TestContext { generation: 1 };
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &setup_context, None),
        &setup,
    ));
    let (root, session) = match outcome {
        SubscriptionSetupOutcome::Ready { root, session } => (root, session),
        SubscriptionSetupOutcome::Response(response) => panic!("{response:?}"),
    };

    assert_eq!(session, "session");
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(*setup.contexts.lock().unwrap(), [1]);
    assert_eq!(root.response_key(), "chosen");
    assert_eq!(root.field_name(), "event");
    assert_eq!(root.arguments().get("id"), Some(&Value::from("room")));
    assert_eq!(*recorder.operations.lock().unwrap(), [(1, 2, 3)]);
    assert_eq!(
        *recorder.fields.lock().unwrap(),
        [(1, "Subscription.event".to_owned(), 1)]
    );

    let first = Event::new("first", "one");
    let first_context = TestContext { generation: 2 };
    let first_response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &first_context, None),
        &root,
        &first,
    ));
    let second = Event::new("second", "two");
    let second_context = TestContext { generation: 3 };
    let second_response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &second_context, None),
        &root,
        &second,
    ));

    assert_eq!(selected_string(&first_response, "required"), Some("first"));
    assert_eq!(selected_string(&first_response, "label"), Some("one"));
    assert_eq!(
        selected_string(&second_response, "required"),
        Some("second")
    );
    assert_eq!(selected_string(&second_response, "label"), Some("two"));
    assert!(first_response.errors.is_empty());
    assert!(second_response.errors.is_empty());
    assert_eq!(*first.contexts.lock().unwrap(), [2]);
    assert_eq!(*second.contexts.lock().unwrap(), [3]);
    assert_eq!(dispatch.root_calls.load(Ordering::SeqCst), 0);
    assert_eq!(dispatch.nested_calls.load(Ordering::SeqCst), 4);
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(recorder.operations.lock().unwrap().len(), 3);
    assert_eq!(recorder.fields.lock().unwrap().len(), 7);
}

#[test]
fn each_event_gets_a_fresh_output_budget() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let setup = Setup::new();
    let plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let mut limits = ExecutionLimits::unbounded();
    limits.max_output_nodes = 3;
    let operation = fixture.operation(limits);
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&dispatch, &plugins, &setup_context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    for generation in [2, 3] {
        let context = TestContext { generation };
        let event = Event::new("ok", "bounded");
        let response = block_on(execute_subscription_event(
            fixture.input(&dispatch, &plugins, &context, None),
            &root,
            &event,
        ));
        assert!(response.errors.is_empty(), "{response:?}");
        assert_eq!(selected_string(&response, "label"), Some("bounded"));
    }
}

#[test]
fn ordinary_nested_completion_masks_and_bubbles_internal_errors() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(true);
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
    let context = TestContext { generation: 2 };
    let event = Event::new("private", "ignored");
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &event,
    ));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(response.errors[0].locations, [Location::new(6, 49)]);
    assert_eq!(
        response.errors[0].path,
        [
            PathSegment::Field("chosen".to_owned()),
            PathSegment::Field("required".to_owned())
        ]
    );
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
    assert_eq!(response.errors[0].extensions.len(), 1);
    assert_eq!(dispatch.root_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn subscription_named_type_nested_under_query_uses_ordinary_dispatch() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let query = "query { nested { event(id: \"room\") { required label } } }";
    let parsed =
        parse_executable_document(SourceId::new(802), query, &ExecutionLimits::unbounded());
    let document = validate_executable_document(
        &fixture.schema,
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap();
    let source = Source::new("query.graphql", query);
    let response = block_on(execute_operation(
        &fixture.schema,
        &document,
        document.select_operation(None).unwrap(),
        &CoercedVariables::new(),
        &fixture.scalars,
        &dispatch,
        &plugins,
        &context,
        &source,
        ExecutionLimits::unbounded(),
        false,
        &fixture.identity,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    assert!(response.errors.is_empty(), "{response:?}");
    assert_eq!(dispatch.root_calls.load(Ordering::SeqCst), 1);
    let Value::Object(data) = response.data.unwrap() else {
        panic!("expected query data")
    };
    let Value::Object(nested) = data.get("nested").unwrap() else {
        panic!("expected nested Subscription object")
    };
    let Value::Object(event) = nested.get("event").unwrap() else {
        panic!("expected ordinary Subscription.event result")
    };
    assert_eq!(event.get("label").and_then(Value::as_str), Some("ordinary"));
}

fn selected_string<'a>(response: &'a crate::GraphQlResponse, field: &str) -> Option<&'a str> {
    let Value::Object(root) = response.data.as_ref()? else {
        return None;
    };
    let Value::Object(event) = root.get("chosen")? else {
        return None;
    };
    event.get(field).and_then(Value::as_str)
}
