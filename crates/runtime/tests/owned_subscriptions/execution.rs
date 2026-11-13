use std::sync::atomic::Ordering;

use chigiri_runtime::__private::{
    PreparedWebSocketOperation, SubscriptionEventResult, SubscriptionSourceState,
    SubscriptionStart, WebSocketOperationKind,
};
use chigiri_runtime::{GraphQlRequest, Value};

use super::fixture::{AppContext, SourceFailure, block_on, fixture, request};

#[test]
fn dynamic_owned_root_uses_setup_once_and_fresh_event_contexts() {
    let (schema, dispatch, calls) = fixture();
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    let prepared = schema.prepare_subscription(request()).unwrap();
    let mut session = match block_on(prepared.start(&AppContext(0), None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("unexpected setup: {response:?}"),
    };
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
    for (generation, value) in [(1, 13), (2, 23)] {
        assert!(matches!(
            block_on(session.next_source(None)),
            SubscriptionSourceState::EventReady
        ));
        let response = match block_on(session.execute_event(&AppContext(generation), None, None)) {
            SubscriptionEventResult::Event(response) => response,
            _ => panic!("owned event was not completed"),
        };
        assert!(response.errors.is_empty(), "{response:?}");
        let Some(Value::Object(root)) = response.data else {
            panic!("missing response data")
        };
        let Some(Value::Object(event)) = root.get("chosen") else {
            panic!("missing event object")
        };
        assert_eq!(event.get("value"), Some(&Value::Int(value)));
        assert_eq!(
            event.get("generation"),
            Some(&Value::Int(generation as i128))
        );
    }
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Complete
    ));
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
    assert_eq!(calls.ordinary_root.load(Ordering::SeqCst), 0);
    assert_eq!(*calls.event_contexts.lock().unwrap(), [1, 2]);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
    assert_eq!(calls.fatal_queries.load(Ordering::SeqCst), 2);
}

#[test]
fn typed_host_failure_terminates_before_the_source_is_restored() {
    let (schema, mut dispatch, calls) = fixture();
    dispatch.host_failure = true;
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    let prepared = schema.prepare_subscription(request()).unwrap();
    let mut session = match block_on(prepared.start(&AppContext(0), None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("unexpected setup: {response:?}"),
    };
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let SubscriptionEventResult::Terminal(response) =
        block_on(session.execute_event(&AppContext(77), None, None))
    else {
        panic!("typed host failure did not terminate immediately")
    };
    assert!(response.data.is_none());
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(calls.fatal_queries.load(Ordering::SeqCst), 1);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
    assert_eq!(calls.fatal_queries.load(Ordering::SeqCst), 1);
}

#[test]
fn ordinary_query_mutation_and_scalar_coercion_are_preserved() {
    let (schema, dispatch, calls) = fixture();
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    for operation in [
        "query { echo(token: \"ok\") }",
        "mutation { set(token: \"ok\") }",
    ] {
        let response = block_on(schema.execute(GraphQlRequest::new(operation), AppContext(7)));
        assert!(response.errors.is_empty(), "{response:?}");
        let Some(Value::Object(data)) = response.data else {
            panic!("missing finite data")
        };
        let field = if operation.starts_with("query") {
            "echo"
        } else {
            "set"
        };
        assert_eq!(data.get(field), Some(&Value::from("ok")));
    }
    assert!(
        schema
            .prepare_subscription(GraphQlRequest::new(
                "subscription { events(shift: 1, token: \"reject\") { value } }"
            ))
            .is_err()
    );
    assert_eq!(calls.setup.load(Ordering::SeqCst), 0);
    let response = block_on(schema.execute(request(), AppContext(7)));
    assert!(!response.errors.is_empty());
    assert_eq!(calls.setup.load(Ordering::SeqCst), 0);
}

#[test]
fn ordinary_owned_binding_still_does_not_enable_streaming() {
    let (schema, dispatch, calls) = fixture();
    let schema = schema.bind(dispatch).unwrap().into_executable();
    assert!(schema.prepare_subscription(request()).is_err());
    assert_eq!(calls.setup.load(Ordering::SeqCst), 0);
}

#[test]
fn websocket_preparation_accepts_the_same_owned_assembly_for_all_operation_kinds() {
    let (schema, dispatch, calls) = fixture();
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    for (request, kind) in [
        (
            GraphQlRequest::new("query { echo(token: \"ok\") }"),
            WebSocketOperationKind::Query,
        ),
        (
            GraphQlRequest::new("mutation { set(token: \"ok\") }"),
            WebSocketOperationKind::Mutation,
        ),
        (request(), WebSocketOperationKind::Subscription),
    ] {
        let prepared = schema.prepare_websocket_operation(request).unwrap();
        assert_eq!(prepared.kind(), kind);
        if let PreparedWebSocketOperation::Subscription(prepared) = prepared {
            let session = block_on(prepared.start(&AppContext(0), None));
            assert!(matches!(session, SubscriptionStart::Ready(_)));
        }
    }
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn source_error_mapping_and_release_use_the_common_session() {
    let (schema, mut dispatch, calls) = fixture();
    dispatch.events = vec![Err(SourceFailure)];
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    let prepared = schema.prepare_subscription(request()).unwrap();
    let mut session = match block_on(prepared.start(&AppContext(0), None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("unexpected setup: {response:?}"),
    };
    let SubscriptionSourceState::Terminal(response) = block_on(session.next_source(None)) else {
        panic!("source error did not terminate")
    };
    assert_eq!(response.errors[0].message, "source stopped");
    assert_eq!(calls.source_maps.load(Ordering::SeqCst), 1);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
    assert!(calls.event_contexts.lock().unwrap().is_empty());
}

#[test]
fn foreign_owned_event_objects_fail_the_existing_scope_check() {
    let (schema, mut dispatch, calls) = fixture();
    dispatch.foreign_event_scope = true;
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    let prepared = schema.prepare_subscription(request()).unwrap();
    let mut session = match block_on(prepared.start(&AppContext(0), None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("unexpected setup: {response:?}"),
    };
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let SubscriptionEventResult::Event(response) =
        block_on(session.execute_event(&AppContext(1), None, None))
    else {
        panic!("missing scope-check response")
    };
    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    drop(session);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
}
