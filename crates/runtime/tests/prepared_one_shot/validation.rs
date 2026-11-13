use chigiri_runtime::{GraphQlRequest, Object, Value};

use super::fixture::{AppContext, block_on, code, named, resolver_calls, scalar_calls, schema};

fn assert_rejected(request: GraphQlRequest, expected: &str, expected_scalar_calls: usize) {
    let (schema, calls) = schema();
    let response = match schema.prepare_one_shot_operation(request) {
        Ok(_) => panic!("request unexpectedly prepared"),
        Err(response) => response,
    };

    assert!(response.data.is_none());
    assert_eq!(code(&response), Some(expected));
    assert_eq!(scalar_calls(&calls), expected_scalar_calls);
    assert_eq!(resolver_calls(&calls), 0);
}

#[test]
fn parse_validation_selection_and_coercion_fail_before_execution() {
    assert_rejected(
        GraphQlRequest::new("query {").with_operation_name("Named"),
        "GRAPHQL_PARSE_FAILED",
        0,
    );
    assert_rejected(
        GraphQlRequest::new("query Named { missing }").with_operation_name("Named"),
        "GRAPHQL_VALIDATION_FAILED",
        0,
    );
    assert_rejected(
        GraphQlRequest::new("query A { fail } query B { fail }"),
        "OPERATION_NOT_FOUND",
        0,
    );
    assert_rejected(
        GraphQlRequest::new("query Named { fail }").with_operation_name("Unknown"),
        "OPERATION_NOT_FOUND",
        0,
    );
    assert_rejected(named("query", "echo", "reject"), "BAD_USER_INPUT", 1);
}

#[test]
fn subscription_is_rejected_before_application_scalar_coercion() {
    let mut variables = Object::new();
    variables.insert("value", Value::from("must-not-coerce"));
    assert_rejected(
        GraphQlRequest::new("subscription Named($value: Probe!) { ticks(value: $value) }")
            .with_operation_name("Named")
            .with_variables(variables),
        "OPERATION_NOT_SUPPORTED",
        0,
    );
}

#[test]
fn finite_and_query_only_keep_subscription_rejection_precedence() {
    let (schema, calls) = schema();
    let request = || {
        let mut variables = Object::new();
        variables.insert("value", Value::from("must-not-coerce"));
        GraphQlRequest::new("subscription Named($value: Probe!) { ticks(value: $value) }")
            .with_operation_name("Named")
            .with_variables(variables)
    };

    let finite = block_on(schema.execute(request(), AppContext { marker: "finite" }));
    let query_only = block_on(schema.execute_query_only(
        request(),
        AppContext {
            marker: "query-only",
        },
    ));

    assert_eq!(code(&finite), Some("OPERATION_NOT_SUPPORTED"));
    assert_eq!(code(&query_only), Some("OPERATION_NOT_SUPPORTED"));
    assert_eq!(scalar_calls(&calls), 0);
    assert_eq!(resolver_calls(&calls), 0);
}
