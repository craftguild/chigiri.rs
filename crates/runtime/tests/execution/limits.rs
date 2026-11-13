use chigiri_runtime::{ExecutionLimits, GraphQlRequest, Value};

use super::{block_on, schema};

fn zero_diagnostic_schema() -> chigiri_runtime::ExecutableSchema<()> {
    let mut limits = ExecutionLimits::production();
    limits.max_diagnostics = 0;
    chigiri_runtime::ExecutableSchema::builder(&super::SCHEMA, super::Dispatch)
        .limits(limits)
        .build()
        .unwrap()
}

fn assert_single_error(response: chigiri_runtime::GraphQlResponse, code: &str) {
    assert!(response.data.is_none());
    assert_eq!(response.errors.len(), 1);
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&Value::String(code.to_owned()))
    );
}

#[test]
fn zero_diagnostic_limit_still_returns_one_parse_error() {
    let response = block_on(zero_diagnostic_schema().execute(GraphQlRequest::new("{ ! }"), ()));

    assert_single_error(response, "GRAPHQL_PARSE_FAILED");
}

#[test]
fn zero_diagnostic_limit_still_returns_one_validation_error() {
    let response =
        block_on(zero_diagnostic_schema().execute(GraphQlRequest::new("{ missing }"), ()));

    assert_single_error(response, "GRAPHQL_VALIDATION_FAILED");
}

#[test]
fn zero_diagnostic_limit_still_returns_one_variable_coercion_error() {
    let request = GraphQlRequest::new("query($value: String!) { echo(value: $value) }");
    let response = block_on(zero_diagnostic_schema().execute(request, ()));

    assert_single_error(response, "BAD_USER_INPUT");
}

#[test]
fn schema_preserves_existing_diagnostics() {
    let response = block_on(schema().execute(GraphQlRequest::new("{ missing }"), ()));

    assert!(!response.errors[0].message.contains("diagnostic limit"));
}

#[test]
fn oversized_operation_names_are_rejected_without_panicking() {
    let name = "a".repeat(256);
    let query = format!("query {name} {{ hello }}");
    let request = GraphQlRequest::new(query).with_operation_name(name);
    let response = block_on(schema().execute(request.clone(), ()));

    assert_single_error(response, "GRAPHQL_VALIDATION_FAILED");
    assert!(schema().validate_operation(&request).is_err());
}

#[test]
fn field_unit_list_overrides_the_masked_error_to_null_the_field() {
    let mut limits = ExecutionLimits::production();
    limits.max_list_items_per_field = 2;
    let schema = chigiri_runtime::ExecutableSchema::builder(&super::SCHEMA, super::Dispatch)
        .limits(limits)
        .build()
        .unwrap();
    let response = block_on(schema.execute(GraphQlRequest::new("{ nullableItems }"), ()));

    let Some(Value::Object(data)) = response.data else {
        panic!("expected a partial data object");
    };
    assert_eq!(data.get("nullableItems"), Some(&Value::Null));
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
}

#[test]
fn request_output_node_limit_applies_to_nested_completion() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 1;
    let schema = chigiri_runtime::ExecutableSchema::builder(&super::SCHEMA, super::Dispatch)
        .limits(limits)
        .build()
        .unwrap();
    let response = block_on(schema.execute(GraphQlRequest::new("{ item { name } }"), ()));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(response.errors[0].path.len(), 2);
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
}
