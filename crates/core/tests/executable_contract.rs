use chigiri_core::executable::{
    CoercedInput, CustomScalarRegistry, EXECUTABLE_FIELD_NOT_DEFINED,
    EXECUTABLE_SUBSCRIPTION_INTROSPECTION_ROOT, EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE,
    EXECUTABLE_SUBSCRIPTION_SINGLE_ROOT_FIELD, EXECUTABLE_VARIABLE_POSITION_INVALID,
    INPUT_CUSTOM_SCALAR_VALUE, ScalarCoercionError, coerce_variable_values,
    validate_executable_document, validate_executable_document_with_scalars,
};
use chigiri_core::language::ast::OperationType;
use chigiri_core::language::{parse_executable_document, parse_type_system_document};
use chigiri_core::schema::{SemanticSchema, compose_schema};
use chigiri_core::{ExecutionLimits, Object, SourceId, Value};

fn schema() -> SemanticSchema {
    let limits = ExecutionLimits::unbounded();
    let parsed = parse_type_system_document(
        SourceId::new(201),
        "type Query { user(id: ID!): User } type User { id: ID! name: String! }",
        &limits,
    );
    compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap()
}

#[test]
fn public_api_supports_separate_validation_selection_and_variable_coercion() {
    let limits = ExecutionLimits::unbounded();
    let parsed = parse_executable_document(
        SourceId::new(202),
        "query GetUser($id: ID!) { user(id: $id) { ...UserFields } } fragment UserFields on User { id name }",
        &limits,
    );
    let output = validate_executable_document(&schema(), &parsed.document.unwrap(), &limits);
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());

    let document = output.document.unwrap();
    assert_eq!(document.operations().count(), 1);
    assert!(document.fragment("UserFields").is_some());
    let selected = document.select_operation(Some("GetUser")).unwrap();
    assert_eq!(selected.operation.operation_type, OperationType::Query);
    assert_eq!(selected.complexity.expanded_fields, 3);

    let mut raw = Object::new();
    raw.insert("id", Value::from("42"));
    let coerced = coerce_variable_values(
        &schema(),
        &selected.operation.variable_definitions,
        raw,
        &CustomScalarRegistry::new(),
        &limits,
    );
    assert!(matches!(
        coerced.variables.unwrap().get("id"),
        Some(CoercedInput::Value(Value::String(value))) if value == "42"
    ));
}

#[test]
fn public_contract_exposes_executable_diagnostic_codes_and_limits() {
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let parsed = parse_executable_document(
        SourceId::new(203),
        "query Q($id: String) { missingA missingB user(id: $id) { id } }",
        &ExecutionLimits::unbounded(),
    );
    let output = validate_executable_document(&schema(), &parsed.document.unwrap(), &limits);

    assert!(!output.is_valid());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_FIELD_NOT_DEFINED
    );
    assert!(output.diagnostics_were_truncated());
    assert_eq!(EXECUTABLE_SUBSCRIPTION_SINGLE_ROOT_FIELD.as_str(), "SF3046");
    assert_eq!(EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE.as_str(), "SF3047");
    assert_eq!(
        EXECUTABLE_SUBSCRIPTION_INTROSPECTION_ROOT.as_str(),
        "SF3048"
    );
    assert_eq!(EXECUTABLE_VARIABLE_POSITION_INVALID.as_str(), "SF3071");
}

#[test]
fn public_api_can_select_custom_scalar_documents_for_validation() {
    let limits = ExecutionLimits::unbounded();
    let parsed_schema = parse_type_system_document(
        SourceId::new(204),
        "scalar Token type Query { check(token: Token): Boolean }",
        &limits,
    );
    let schema = compose_schema(&[parsed_schema.document.unwrap()], &limits)
        .schema
        .unwrap();
    let accepted = parse_executable_document(
        SourceId::new(205),
        r#"query Q { check(token: "good") }"#,
        &limits,
    )
    .document
    .unwrap();
    let rejected = parse_executable_document(
        SourceId::new(206),
        r#"query Q { check(token: "bad") }"#,
        &limits,
    )
    .document
    .unwrap();
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("Token", |_, value| {
        if value.as_str() == Some("bad") {
            Err(ScalarCoercionError::public("bad token"))
        } else {
            Ok(value)
        }
    });

    let output = validate_executable_document_with_scalars(&schema, &accepted, &scalars, &limits);
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let output = validate_executable_document_with_scalars(&schema, &rejected, &scalars, &limits);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        INPUT_CUSTOM_SCALAR_VALUE
    );
    assert_eq!(
        output.diagnostics.iter().next().unwrap().message,
        "bad token"
    );

    let strict = validate_executable_document(&schema, &accepted, &limits);
    assert_eq!(
        strict.diagnostics.iter().next().unwrap().code,
        INPUT_CUSTOM_SCALAR_VALUE
    );
}
