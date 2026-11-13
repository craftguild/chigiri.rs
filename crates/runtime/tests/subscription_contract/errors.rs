use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{
    SubscriptionEventResult, SubscriptionSourceState, SubscriptionStart,
};
use chigiri_runtime::{ErrorEvent, ErrorObserver, GraphQlResponse, PathSegment, Value};

use super::fixture::{
    AppContext, EventMode, SetupMode, SourceStep, State, block_on, builder, count, request,
    request_fields, schema, start,
};

#[derive(Clone, Default)]
struct ErrorRecorder(Arc<Mutex<Vec<String>>>);

impl ErrorObserver for ErrorRecorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0.lock().unwrap().push(event.source().to_string());
    }
}

#[test]
fn outer_setup_mapper_runs_once_and_stops_before_source_creation() {
    let state = State::modes(Vec::new(), SetupMode::OuterError, EventMode::Object);
    let schema = schema(Arc::clone(&state));
    let context = AppContext::new(0);
    let prepared = schema.prepare_subscription(request()).unwrap();
    let response = match block_on(prepared.start(&context, None)) {
        SubscriptionStart::Response(response) => response,
        SubscriptionStart::Ready(_) => panic!("outer error unexpectedly established a source"),
    };

    assert_eq!(count(&state.counts.outer_maps), 1);
    assert_eq!(count(&state.counts.source_maps), 0);
    assert_eq!(count(&state.counts.event_maps), 0);
    assert_eq!(response.errors[0].message, "outer setup failure");
    assert_eq!(error_code(&response), Some("OUTER_FAILURE"));
}

#[test]
fn source_completion_is_distinct_from_a_single_mapped_terminal_error() {
    let complete_state = State::new(Vec::new());
    let complete_schema = schema(Arc::clone(&complete_state));
    let context = AppContext::new(0);
    let mut complete = start(&complete_schema, &context);
    assert!(matches!(
        block_on(complete.next_source(None)),
        SubscriptionSourceState::Complete
    ));
    assert_eq!(count(&complete_state.counts.source_maps), 0);

    let state = State::new(vec![SourceStep::Error]);
    let recorder = ErrorRecorder::default();
    let schema = builder(Arc::clone(&state))
        .error_observer(recorder.clone())
        .build()
        .unwrap();
    let mut session = start(&schema, &context);
    let response = match block_on(session.next_source(None)) {
        SubscriptionSourceState::Terminal(response) => response,
        _ => panic!("source error did not terminate the session"),
    };

    assert!(response.data.is_none());
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(error_code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(response.errors[0].path, [PathSegment::from("chosen")]);
    assert_eq!(count(&state.counts.source_maps), 1);
    assert_eq!(count(&state.counts.outer_maps), 0);
    assert_eq!(count(&state.counts.event_maps), 0);
    assert_eq!(*recorder.0.lock().unwrap(), ["source-secret"]);
    assert!(!format!("{response:?}").contains("source-secret"));
}

#[test]
fn event_mapper_runs_once_and_returns_a_masked_graphql_event() {
    let state = State::modes(
        vec![SourceStep::Event(1)],
        SetupMode::Ready,
        EventMode::EventError,
    );
    let recorder = ErrorRecorder::default();
    let schema = builder(Arc::clone(&state))
        .error_observer(recorder.clone())
        .build()
        .unwrap();
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let event = AppContext::new(1);
    let response = event_response(block_on(session.execute_event(&event, None, None)));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(error_code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(count(&state.counts.event_maps), 1);
    assert_eq!(count(&state.counts.outer_maps), 0);
    assert_eq!(count(&state.counts.source_maps), 0);
    assert_eq!(*recorder.0.lock().unwrap(), ["event-secret"]);
    assert!(!format!("{response:?}").contains("event-secret"));
}

#[test]
fn ordinary_nested_errors_preserve_partial_data_and_non_null_bubbling() {
    let partial_state = State::new(vec![SourceStep::Event(3)]);
    let partial_schema = schema(Arc::clone(&partial_state));
    let setup = AppContext::new(0);
    let prepared = partial_schema
        .prepare_subscription(request_fields("value optionalFail"))
        .unwrap();
    let mut session = ready(block_on(prepared.start(&setup, None)));
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let event = AppContext::new(1);
    let partial = event_response(block_on(session.execute_event(&event, None, None)));
    let Value::Object(root) = partial.data.as_ref().unwrap() else {
        panic!("expected partial root object");
    };
    let Value::Object(data) = root.get("chosen").unwrap() else {
        panic!("expected partial event object");
    };
    assert_eq!(data.get("value"), Some(&Value::Int(3)));
    assert_eq!(data.get("optionalFail"), Some(&Value::Null));
    assert_eq!(partial.errors.len(), 1);
    assert_eq!(error_code(&partial), Some("NESTED_FAILURE"));

    let bubble_state = State::new(vec![SourceStep::Event(4)]);
    let bubble_schema = schema(Arc::clone(&bubble_state));
    let prepared = bubble_schema
        .prepare_subscription(request_fields("value requiredFail"))
        .unwrap();
    let mut session = ready(block_on(prepared.start(&setup, None)));
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let bubbled = event_response(block_on(session.execute_event(&event, None, None)));
    assert_eq!(bubbled.data, Some(Value::Null));
    assert_eq!(bubbled.errors.len(), 1);
    assert_eq!(error_code(&bubbled), Some("NESTED_FAILURE"));
    assert_eq!(count(&bubble_state.counts.ordinary_root_calls), 0);
}

fn ready(
    start: SubscriptionStart<AppContext>,
) -> chigiri_runtime::__private::SubscriptionSession<AppContext> {
    match start {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("setup failed: {response:?}"),
    }
}

fn event_response(result: SubscriptionEventResult) -> GraphQlResponse {
    match result {
        SubscriptionEventResult::Event(response) => response,
        SubscriptionEventResult::Terminal(response) => panic!("terminal event: {response:?}"),
        SubscriptionEventResult::Cancelled => panic!("event was cancelled"),
    }
}

fn error_code(response: &GraphQlResponse) -> Option<&str> {
    response
        .errors
        .first()?
        .extensions
        .get("code")
        .and_then(Value::as_str)
}
