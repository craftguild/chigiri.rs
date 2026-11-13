use std::sync::Arc;

use chigiri_runtime::__private::{BoxFuture, SubscriptionEventResult, SubscriptionSourceState};
use chigiri_runtime::{
    ExecutionLimits, GraphQlResponse, OperationContext, OperationNext, Plugin, Value,
};

use super::fixture::{
    AppContext, EventMode, SetupMode, SourceStep, State, block_on, builder, count, schema, start,
};

struct StripEventErrors;

impl Plugin<AppContext> for StripEventErrors {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, AppContext>,
        next: OperationNext<'a, AppContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let mut response = next.run(context).await;
            response.errors.clear();
            response
        })
    }
}

#[test]
fn resource_failure_is_terminal_even_when_a_plugin_removes_response_errors() {
    let state = State::new(vec![SourceStep::Event(1), SourceStep::Event(2)]);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_output_nodes = 1;
    let schema = builder(Arc::clone(&state))
        .limits(limits)
        .plugin(StripEventErrors)
        .build()
        .unwrap();
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    demand_one(&mut session);
    let event = AppContext::new(1);

    let response = terminal(block_on(session.execute_event(&event, None, None)));

    assert!(response.data.is_none());
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(error_code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(count(&state.counts.source_polls), 1);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(count(&state.counts.event_drops), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

#[test]
fn generated_event_invariant_is_terminal_and_releases_the_source() {
    let state = State::modes(
        vec![SourceStep::Event(1)],
        SetupMode::Ready,
        EventMode::FrameworkInvariant,
    );
    let schema = schema(Arc::clone(&state));
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    demand_one(&mut session);
    let event = AppContext::new(1);

    let response = terminal(block_on(session.execute_event(&event, None, None)));

    assert!(response.data.is_none());
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(error_code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(count(&state.counts.event_maps), 0);
}

#[test]
fn ordinary_mapped_field_errors_remain_events_with_fresh_failure_flags() {
    let state = State::modes(
        vec![SourceStep::Event(1), SourceStep::Event(2)],
        SetupMode::Ready,
        EventMode::EventError,
    );
    let schema = schema(Arc::clone(&state));
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);

    for generation in [1, 2] {
        demand_one(&mut session);
        let context = AppContext::new(generation);
        let response = event(block_on(session.execute_event(&context, None, None)));
        assert_eq!(response.data, Some(Value::Null));
        assert_eq!(response.errors[0].message, "Internal server error");
    }
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Complete
    ));
    assert_eq!(count(&state.counts.event_maps), 2);
    assert_eq!(count(&state.counts.source_drops), 1);
}

fn demand_one(session: &mut chigiri_runtime::__private::SubscriptionSession<AppContext>) {
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
}

fn terminal(result: SubscriptionEventResult) -> GraphQlResponse {
    match result {
        SubscriptionEventResult::Terminal(response) => response.into_response(),
        SubscriptionEventResult::Event(response) => {
            panic!("framework failure continued: {response:?}")
        }
        SubscriptionEventResult::Cancelled => panic!("framework failure was cancelled"),
    }
}

fn event(result: SubscriptionEventResult) -> GraphQlResponse {
    match result {
        SubscriptionEventResult::Event(response) => response,
        SubscriptionEventResult::Terminal(response) => {
            panic!("ordinary error terminated the source: {response:?}")
        }
        SubscriptionEventResult::Cancelled => panic!("ordinary event was cancelled"),
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
