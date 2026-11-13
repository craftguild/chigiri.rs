use crate::executable::{
    EXECUTABLE_DUPLICATE_VARIABLE, EXECUTABLE_VARIABLE_LIMIT, EXECUTABLE_VARIABLE_NOT_INPUT,
    EXECUTABLE_VARIABLE_UNDEFINED, EXECUTABLE_VARIABLE_UNUSED, validate_executable_document,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{ExecutionLimits, SourceId};

fn validate(source: &str, limits: &ExecutionLimits) -> super::ExecutableValidationOutput {
    let schema_document = parse_type_system_document(
        SourceId::new(20),
        "type Query { find(id: ID): String } type User { id: ID }",
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap();
    let schema = compose_schema(&[schema_document], &ExecutionLimits::unbounded())
        .schema
        .unwrap();
    let document = parse_executable_document(SourceId::new(21), source, limits)
        .document
        .unwrap();
    validate_executable_document(&schema, &document, limits)
}

#[test]
fn distinguishes_duplicate_non_input_undefined_and_unused_variables() {
    let output = validate(
        "query Find($id: ID, $id: ID, $user: User, $unused: String) { find(id: $missing) }",
        &ExecutionLimits::unbounded(),
    );
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&EXECUTABLE_DUPLICATE_VARIABLE));
    assert!(codes.contains(&EXECUTABLE_VARIABLE_NOT_INPUT));
    assert!(codes.contains(&EXECUTABLE_VARIABLE_UNDEFINED));
    assert!(codes.contains(&EXECUTABLE_VARIABLE_UNUSED));
}

#[test]
fn attributes_fragment_variable_use_to_the_operation() {
    let output = validate(
        "query Find($id: ID) { ...Lookup } fragment Lookup on Query { find(id: $id) }",
        &ExecutionLimits::unbounded(),
    );

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn limits_variable_definitions_per_operation() {
    let mut limits = ExecutionLimits::unbounded();
    limits.max_variables = 1;
    let output = validate("query Find($a: ID, $b: ID) { find(id: $a) }", &limits);

    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_VARIABLE_LIMIT
    );
    assert!(output.document.is_none());
}
