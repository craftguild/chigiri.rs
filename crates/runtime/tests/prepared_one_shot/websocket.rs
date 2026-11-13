use std::sync::atomic::Ordering;

use chigiri_runtime::__private::{PreparedWebSocketOperation, WebSocketOperationKind};
use chigiri_runtime::{GraphQlRequest, Object, Value};

use super::fixture::{
    AppContext, block_on, code, many, named, object_leaf, resolver_calls, scalar_calls, schema,
};

#[test]
fn unified_checkpoint_fixes_query_and_mutation_kind_before_execution() {
    let (schema, calls) = schema();
    let query = schema
        .prepare_websocket_operation(named("query", "echo", "query-value"))
        .unwrap();
    let mutation = schema
        .prepare_websocket_operation(named("mutation", "set", 41_i32))
        .unwrap();

    assert_eq!(query.kind(), WebSocketOperationKind::Query);
    assert_eq!(mutation.kind(), WebSocketOperationKind::Mutation);
    assert_eq!(scalar_calls(&calls), 2);
    assert_eq!(resolver_calls(&calls), 0);
    assert_eq!(calls.request_plugin.load(Ordering::SeqCst), 0);

    let PreparedWebSocketOperation::OneShot(query) = query else {
        panic!("query checkpoint changed kind")
    };
    let PreparedWebSocketOperation::OneShot(mutation) = mutation else {
        panic!("mutation checkpoint changed kind")
    };
    let query = block_on(query.execute(AppContext { marker: "ws-query" }, None));
    let mutation = block_on(mutation.execute(
        AppContext {
            marker: "ws-mutation",
        },
        None,
    ));

    assert_eq!(
        object_leaf(&query, "echo"),
        Some(&Value::from("query-value"))
    );
    assert_eq!(object_leaf(&mutation, "set"), Some(&Value::Int(41)));
    assert_eq!(scalar_calls(&calls), 2);
    assert_eq!(resolver_calls(&calls), 2);
    assert_eq!(*calls.contexts.lock().unwrap(), ["ws-query", "ws-mutation"]);
}

#[test]
fn missing_subscription_dispatch_rejects_before_application_scalar() {
    let (schema, calls) = schema();
    let mut variables = Object::new();
    variables.insert("value", Value::from("must-not-coerce"));
    let request =
        GraphQlRequest::new("subscription Named($value: Probe!) { ticks(value: $value) }")
            .with_operation_name("Named")
            .with_variables(variables);

    let response = match schema.prepare_websocket_operation(request) {
        Ok(_) => panic!("subscription prepared without generated dispatch"),
        Err(response) => response,
    };

    assert_eq!(code(&response), Some("OPERATION_NOT_SUPPORTED"));
    assert_eq!(scalar_calls(&calls), 0);
    assert_eq!(resolver_calls(&calls), 0);
}

#[test]
fn unified_checkpoint_carries_the_remaining_input_budget_into_execution() {
    let (schema, calls) = schema();
    let prepared = schema.prepare_websocket_operation(many(50_000)).unwrap();
    let PreparedWebSocketOperation::OneShot(prepared) = prepared else {
        panic!("query checkpoint changed kind")
    };

    let response = block_on(prepared.execute(AppContext { marker: "budget" }, None));

    assert_eq!(code(&response), Some("BAD_USER_INPUT"));
    assert_eq!(resolver_calls(&calls), 0);
}
