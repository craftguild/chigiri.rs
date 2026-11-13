//! Test depth, list, node, and diagnostic limits inside custom scalars.

use chigiri::{ExecutionLimits, GraphQlRequest, GraphQlResponse, PathSegment, Value};

use crate::fixture::{block_on, gated_schema, observed_schema, schema};

#[test]
fn cso_04_mask_nested_numeric_and_invalid_value_not_reserved_node() {
    for mode in ["nan", "bad-bigint"] {
        let (executable, recorder) = observed_schema(ExecutionLimits::production());
        let response = block_on(executable.execute(request(mode), ()));
        assert_masked(&response, &[PathSegment::from("json")]);
        let records = recorder.records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].source,
            "The custom-scalar output has an invalid numeric representation."
        );
        assert_eq!(records[0].coordinate.as_deref(), Some("Query.json"));
        assert_eq!(records[0].component, "completion");
        let debug = format!("{response:?}");
        assert!(!debug.contains("01-private"));
        assert!(!debug.contains("NaN"));
    }

    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 3;
    let response = block_on(schema(limits).execute(
        GraphQlRequest::new(concat!(
            "{ invalid: json(mode: \"bad-bigint\") ",
            "valid: json(mode: \"one-member\") }"
        )),
        (),
    ));
    let data = response.data.unwrap().as_object().unwrap().clone();
    assert_eq!(data.get("invalid"), Some(&Value::Null));
    assert!(matches!(data.get("valid"), Some(Value::Object(_))));
}

#[test]
fn cso_05_accepts_depth_zero_and_rejects_values_above_the_maximum() {
    let mut zero = ExecutionLimits::production();
    zero.max_custom_scalar_output_depth = 0;
    let response = block_on(schema(zero).execute(
        GraphQlRequest::new(concat!(
            "{ primitive: json(mode: \"string\") ",
            "empty: json(mode: \"empty-object\") }"
        )),
        (),
    ));
    assert!(response.errors.is_empty());

    let mut exact = ExecutionLimits::production();
    exact.max_custom_scalar_output_depth = 2;
    let exact_response = block_on(schema(exact).execute(request("depth-2"), ()));
    assert!(exact_response.errors.is_empty());

    let mut exceeded = exact;
    exceeded.max_custom_scalar_output_depth = 1;
    let exceeded_response = block_on(schema(exceeded).execute(request("depth-2"), ()));
    assert_masked(&exceeded_response, &[PathSegment::from("json")]);
}

#[test]
fn cso_06_drops_deep_invalid_output_without_stack_overflow() {
    let mut limits = ExecutionLimits::production();
    limits.max_custom_scalar_output_depth = 10_000;
    let response = block_on(schema(limits).execute(request("deep-invalid"), ()));

    assert_masked(&response, &[PathSegment::from("json")]);
    let debug = format!("{response:?}");
    assert!(!debug.contains("01-deep-private"));
    assert!(!debug.contains("10000"));
}

#[test]
fn cso_07_08_enforces_empty_and_nested_node_boundaries_without_root_nodes() {
    let mut one = ExecutionLimits::production();
    one.max_output_nodes = 1;
    assert!(execute_mode("string", one).errors.is_empty());
    assert!(execute_mode("empty-object", one).errors.is_empty());
    for mode in ["one-member", "one-item"] {
        assert_masked(&execute_mode(mode, one), &[PathSegment::from("json")]);
    }

    let mut two = one;
    two.max_output_nodes = 2;
    for mode in ["one-member", "one-item"] {
        assert!(execute_mode(mode, two).errors.is_empty());
    }

    let mut exact = one;
    exact.max_output_nodes = 4;
    assert!(execute_mode("nested-nodes", exact).errors.is_empty());
    exact.max_output_nodes = 3;
    assert_masked(
        &execute_mode("nested-nodes", exact),
        &[PathSegment::from("json")],
    );
}

#[test]
fn applies_the_same_item_limit_to_root_and_nested_lists() {
    for mode in ["root-list-3", "nested-list-3"] {
        let mut exact = ExecutionLimits::production();
        exact.max_list_items_per_field = 3;
        assert!(execute_mode(mode, exact).errors.is_empty());

        exact.max_list_items_per_field = 2;
        assert_masked(&execute_mode(mode, exact), &[PathSegment::from("json")]);
    }
}

#[test]
fn cso_10_concurrent_sibling_does_not_oversubscribe_to_global_node_budget() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 3;
    let response = block_on(gated_schema(limits).execute(
        GraphQlRequest::new(concat!(
            "{ left: json(mode: \"gated-left\") ",
            "right: json(mode: \"gated-right\") }"
        )),
        (),
    ));

    assert_eq!(response.errors.len(), 1);
    let data = response.data.unwrap();
    let data = data.as_object().unwrap();
    let values = [data.get("left").unwrap(), data.get("right").unwrap()];
    assert_eq!(values.iter().filter(|value| value.is_null()).count(), 1);
    assert_eq!(
        values
            .iter()
            .filter(|value| matches!(value, Value::Object(_)))
            .count(),
        1
    );
}

#[test]
fn custom_scalar_null_bubbling_matches_standard_null_bubbling() {
    let nullable = execute_mode("nan", ExecutionLimits::production());
    assert_eq!(
        nullable
            .data
            .as_ref()
            .unwrap()
            .as_object()
            .unwrap()
            .get("json"),
        Some(&Value::Null)
    );
    assert_eq!(nullable.errors.len(), 1);

    let required = block_on(
        schema(ExecutionLimits::production())
            .execute(GraphQlRequest::new("{ requiredJson(mode: \"nan\") }"), ()),
    );
    assert_eq!(required.data, Some(Value::Null));
    assert_eq!(required.errors.len(), 1);
}

#[test]
fn cso_12_15_direct_execution_diagnoses_and_serializes_field_errors_at_existing_paths() {
    let (executable, recorder) = observed_schema(ExecutionLimits::production());
    let response = block_on(executable.execute(request("bad-bigint"), ()));
    assert_masked(&response, &[PathSegment::from("json")]);
    let response_id = response.errors[0]
        .extensions
        .get("errorId")
        .and_then(Value::as_str);
    let records = recorder.records();
    assert_eq!(records[0].error_id.as_deref(), response_id);
    assert_eq!(records[0].path, [PathSegment::from("json")]);

    let mut limits = ExecutionLimits::production();
    limits.max_custom_scalar_output_depth = 1;
    let (executable, recorder) = observed_schema(limits);
    let limited = block_on(executable.execute(request("depth-2"), ()));
    assert_masked(&limited, &[PathSegment::from("json")]);
    assert!(!format!("{limited:?}").contains("response output limit exceeded"));
    let response_id = limited.errors[0]
        .extensions
        .get("errorId")
        .and_then(Value::as_str);
    let records = recorder.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].error_id.as_deref(), response_id);
    assert_eq!(records[0].source, "response output limit exceeded");
    assert_eq!(records[0].path, [PathSegment::from("json")]);
    assert_eq!(records[0].coordinate.as_deref(), Some("Query.json"));
    assert_eq!(records[0].component, "completion");

    let (executable, recorder) = observed_schema(ExecutionLimits::production());
    let failure = block_on(executable.execute(request("serialize-error"), ()));
    assert_masked(&failure, &[PathSegment::from("json")]);
    let records = recorder.records();
    assert_eq!(records[0].source, "synthetic scalar serialize failure");
    assert_eq!(records[0].component, "resolver");
    assert_eq!(chigiri::GENERATED_ABI_VERSION, 8);
}

fn execute_mode(mode: &str, limits: ExecutionLimits) -> GraphQlResponse {
    block_on(schema(limits).execute(request(mode), ()))
}

fn request(mode: &str) -> GraphQlRequest {
    GraphQlRequest::new(format!("{{ json(mode: \"{mode}\") }}"))
}

fn assert_masked(response: &GraphQlResponse, path: &[PathSegment]) {
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(response.errors[0].path, path);
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
}
