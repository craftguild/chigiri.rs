use std::sync::atomic::Ordering;

use chigiri_runtime::__private::OneShotOperationKind;
use chigiri_runtime::{GraphQlRequest, Value};

use super::fixture::{
    AppContext, assert_kind, block_on, code, many, named, object_leaf, resolver_calls,
    scalar_calls, schema,
};

#[test]
fn named_query_is_fully_prepared_before_fresh_context_execution() {
    let (schema, calls) = schema();
    let prepared = schema
        .prepare_one_shot_operation(named("query", "echo", "query-value"))
        .unwrap();

    assert_kind(prepared.kind(), OneShotOperationKind::Query);
    assert_eq!(
        prepared
            .identity()
            .key()
            .operation_name()
            .map(|name| name.as_str()),
        Some("Named")
    );
    assert_eq!(scalar_calls(&calls), 1);
    assert_eq!(resolver_calls(&calls), 0);
    assert_eq!(calls.request_plugin.load(Ordering::SeqCst), 0);

    let response = block_on(prepared.execute(AppContext { marker: "query" }, None));

    assert!(response.errors.is_empty());
    assert_eq!(
        object_leaf(&response, "echo"),
        Some(&Value::from("query-value"))
    );
    assert_eq!(scalar_calls(&calls), 1);
    assert_eq!(resolver_calls(&calls), 1);
    assert_eq!(calls.request_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(calls.operation_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(calls.field_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(*calls.contexts.lock().unwrap(), ["query"]);
}

#[test]
fn named_mutation_and_anonymous_query_keep_the_selected_kind_and_variables() {
    let (schema, calls) = schema();
    let mutation = schema
        .prepare_one_shot_operation(named("mutation", "set", 41_i32))
        .unwrap();
    let anonymous = schema
        .prepare_one_shot_operation(GraphQlRequest::new("{ constant }"))
        .unwrap();

    assert_kind(mutation.kind(), OneShotOperationKind::Mutation);
    assert_kind(anonymous.kind(), OneShotOperationKind::Query);
    assert!(anonymous.identity().key().operation_name().is_none());
    assert_eq!(scalar_calls(&calls), 1);
    assert_eq!(resolver_calls(&calls), 0);

    let mutation = block_on(mutation.execute(AppContext { marker: "mutation" }, None));
    let anonymous = block_on(anonymous.execute(
        AppContext {
            marker: "anonymous",
        },
        None,
    ));

    assert_eq!(object_leaf(&mutation, "set"), Some(&Value::Int(41)));
    assert_eq!(object_leaf(&anonymous, "constant"), Some(&Value::Int(7)));
    assert_eq!(scalar_calls(&calls), 1);
    assert_eq!(resolver_calls(&calls), 2);
    assert_eq!(*calls.contexts.lock().unwrap(), ["mutation", "anonymous"]);
}

#[test]
fn existing_plugins_and_internal_error_mapping_run_once_at_execution() {
    let (schema, calls) = schema();
    let prepared = schema
        .prepare_one_shot_operation(
            GraphQlRequest::new("query Named { fail }").with_operation_name("Named"),
        )
        .unwrap();
    assert_eq!(resolver_calls(&calls), 0);

    let response = block_on(prepared.execute(AppContext { marker: "failure" }, None));

    assert!(response.data.is_some());
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert!(!format!("{:?}", response.errors[0]).contains("resolver-secret"));
    assert_eq!(resolver_calls(&calls), 1);
    assert_eq!(calls.request_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(calls.operation_plugin.load(Ordering::SeqCst), 1);
    assert_eq!(calls.field_plugin.load(Ordering::SeqCst), 1);
}

#[test]
fn execution_continues_the_preparation_input_budget_instead_of_resetting_it() {
    let (schema, calls) = schema();
    let prepared = schema.prepare_one_shot_operation(many(50_000)).unwrap();
    assert_eq!(resolver_calls(&calls), 0);

    let response = block_on(prepared.execute(AppContext { marker: "budget" }, None));

    assert_eq!(code(&response), Some("BAD_USER_INPUT"));
    assert_eq!(resolver_calls(&calls), 0);
}
