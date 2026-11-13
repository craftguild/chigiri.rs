use std::sync::Arc;

use chigiri_runtime::__private::{PreparedWebSocketOperation, WebSocketOperationKind};
use chigiri_runtime::GraphQlRequest;

use super::fixture::{State, count, request, schema};

#[test]
fn unified_checkpoint_selects_subscription_without_setup_or_prefetch() {
    let state = State::new(Vec::new());
    let schema = schema(Arc::clone(&state));

    let prepared = schema.prepare_websocket_operation(request()).unwrap();

    assert_eq!(prepared.kind(), WebSocketOperationKind::Subscription);
    assert!(matches!(
        prepared,
        PreparedWebSocketOperation::Subscription(_)
    ));
    assert_eq!(count(&state.counts.setup_calls), 0);
    assert_eq!(count(&state.counts.source_polls), 0);
}

#[test]
fn unified_checkpoint_selects_query_on_the_same_generated_schema() {
    let state = State::new(Vec::new());
    let schema = schema(Arc::clone(&state));
    let prepared = schema
        .prepare_websocket_operation(GraphQlRequest::new("query { ping }"))
        .unwrap();

    assert_eq!(prepared.kind(), WebSocketOperationKind::Query);
    let PreparedWebSocketOperation::OneShot(_) = prepared else {
        panic!("query checkpoint changed kind")
    };
    assert_eq!(count(&state.counts.setup_calls), 0);
    assert_eq!(count(&state.counts.source_polls), 0);
}
