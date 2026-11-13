use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_runtime::__private::{
    SubscriptionEventResult, SubscriptionSourceState, SubscriptionStart,
};
use chigiri_runtime::{ExecutionLimits, GraphQlRequest, SubscriptionOverflow, Value};

use super::fixture::{
    AppContext, SourceStep, State, block_on, builder, count, request, schema, start,
};

#[test]
fn validation_and_variable_coercion_finish_before_setup_or_finite_execution() {
    let state = State::new(Vec::new());
    let schema = schema(Arc::clone(&state));
    let invalid = GraphQlRequest::new(
        "subscription Named($limit: Int!) { chosen: events(limit: $limit) { missing } }",
    )
    .with_operation_name("Named");
    assert!(schema.prepare_subscription(invalid).is_err());

    let missing_variable = GraphQlRequest::new(
        "subscription Named($limit: Int!) { chosen: events(limit: $limit) { value } }",
    )
    .with_operation_name("Named");
    assert!(schema.prepare_subscription(missing_variable).is_err());
    assert_eq!(count(&state.counts.setup_calls), 0);

    let prepared = schema.prepare_subscription(request()).unwrap();
    assert_eq!(count(&state.counts.setup_calls), 0);
    drop(prepared);

    let response = block_on(schema.execute(request(), AppContext::new(99)));
    assert!(response.data.is_none());
    assert!(!response.errors.is_empty());
    assert_eq!(count(&state.counts.setup_calls), 0);
    assert_eq!(count(&state.counts.ordinary_root_calls), 0);
}

#[test]
fn setup_runs_once_and_does_not_retain_its_application_context() {
    let state = State::new(vec![SourceStep::Event(7)]);
    let schema = schema(Arc::clone(&state));
    let drops = Arc::new(AtomicUsize::new(0));
    let context = AppContext::with_shared_drops(10, Arc::clone(&drops));
    let prepared = schema.prepare_subscription(request()).unwrap();

    let session = match block_on(prepared.start(&context, None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("setup failed: {response:?}"),
    };
    assert_eq!(count(&state.counts.setup_calls), 1);
    assert_eq!(*state.setup_generations.lock().unwrap(), [10]);
    assert_eq!(session.overflow(), SubscriptionOverflow::DropOldest);
    assert!(session.identity().key().operation_name().is_some());
    assert_eq!(count(&state.counts.source_polls), 0);

    drop(context);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(session);
    assert_eq!(count(&state.counts.source_drops), 1);
}

#[test]
fn two_events_reuse_arguments_but_use_fresh_contexts_without_prefetch() {
    let state = State::new(vec![SourceStep::Event(10), SourceStep::Event(20)]);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_output_nodes = 2;
    let schema = builder(Arc::clone(&state)).limits(limits).build().unwrap();
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    drop(setup);

    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    assert_eq!(count(&state.counts.source_polls), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    assert_eq!(count(&state.counts.source_polls), 1);

    let first = AppContext::new(1);
    let response = event_response(block_on(session.execute_event(&first, None, None)));
    assert_eq!(response_value(&response), Some(10));
    assert_eq!(count(&state.counts.source_polls), 1);
    drop(first);

    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let second = AppContext::new(2);
    let response = event_response(block_on(session.execute_event(&second, None, None)));
    assert_eq!(response_value(&response), Some(20));
    drop(second);

    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Complete
    ));
    assert_eq!(count(&state.counts.source_polls), 3);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(count(&state.counts.event_drops), 2);
    assert_eq!(count(&state.counts.ordinary_root_calls), 0);
    assert_eq!(count(&state.counts.nested_calls), 2);
    assert_eq!(*state.event_generations.lock().unwrap(), [1, 2]);
    assert_eq!(*state.retained_arguments.lock().unwrap(), [2, 2]);
}

#[test]
fn executing_without_a_demanded_event_is_terminal_and_releases_the_source() {
    let state = State::new(vec![SourceStep::Event(1)]);
    let schema = schema(Arc::clone(&state));
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    let event = AppContext::new(1);

    let response = match block_on(session.execute_event(&event, None, None)) {
        SubscriptionEventResult::Terminal(response) => response,
        _ => panic!("missing event did not terminate the session"),
    };

    assert!(response.data.is_none());
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(count(&state.counts.source_polls), 0);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(count(&state.counts.event_maps), 0);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

fn event_response(result: SubscriptionEventResult) -> chigiri_runtime::GraphQlResponse {
    match result {
        SubscriptionEventResult::Event(response) => response,
        SubscriptionEventResult::Terminal(response) => panic!("terminal event: {response:?}"),
        SubscriptionEventResult::Cancelled => panic!("event was cancelled"),
    }
}

fn response_value(response: &chigiri_runtime::GraphQlResponse) -> Option<i128> {
    let Value::Object(root) = response.data.as_ref()? else {
        return None;
    };
    let Value::Object(event) = root.get("chosen")? else {
        return None;
    };
    let Value::Int(value) = event.get("value")? else {
        return None;
    };
    Some(*value)
}
