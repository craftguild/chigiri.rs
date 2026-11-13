use crate::executable::{
    CustomScalarRegistry, INPUT_NESTING_LIMIT, INPUT_REQUIRED_VALUE_MISSING, INPUT_SCALAR_VALUE,
    coerce_raw_input, coerce_variable_values,
};
use crate::language::ast::{Definition, ExecutableDefinition};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, Object, SourceId, Value};

fn schema() -> SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(41),
        "scalar Token type Query { token: Token, count(value: Int): Boolean }",
        &ExecutionLimits::unbounded(),
    );
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

#[test]
fn raw_input_nesting_failure_remains_invalid_when_diagnostic_limit_is_zero() {
    let schema = schema();
    let expected = &schema.type_by_name("Token").unwrap();
    let expected = crate::language::ast::TypeRef::named(
        crate::language::ast::Name::new(
            expected.name().to_owned(),
            crate::Span::new(SourceId::new(41), 0, 5),
        ),
        crate::Span::new(SourceId::new(41), 0, 5),
    );
    let nested = Value::List(vec![Value::List(vec![Value::List(vec![Value::Int(1)])])]);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_parser_nesting = 1;
    limits.max_diagnostics = 0;
    let output = coerce_raw_input(
        &schema,
        &expected,
        nested,
        &CustomScalarRegistry::new(),
        &limits,
    );
    assert!(!output.is_valid());
    assert!(output.diagnostics.is_empty());
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn raw_input_reports_one_error_and_marks_follow_up_diagnostics_truncated() {
    let schema = schema();
    let expected = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("count", "value")
        .unwrap()
        .ty;
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = coerce_raw_input(
        &schema,
        expected,
        Value::String("not an integer".to_owned()),
        &CustomScalarRegistry::new(),
        &limits,
    );

    assert!(!output.is_valid());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        INPUT_SCALAR_VALUE
    );
    assert!(output.diagnostics_were_truncated());
    assert_ne!(
        output.diagnostics.iter().next().unwrap().code,
        INPUT_NESTING_LIMIT
    );
}

#[test]
fn variable_entry_limit_reports_the_first_overflow_and_stops() {
    let schema = schema();
    let parsed = parse_executable_document(
        SourceId::new(43),
        "query Q($first: Int!, $second: Int!) { __typename }",
        &ExecutionLimits::unbounded(),
    );
    let document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation definition");
    };
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = coerce_variable_values(
        &schema,
        &operation.variable_definitions,
        Object::new(),
        &CustomScalarRegistry::new(),
        &limits,
    );

    assert!(!output.is_valid());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        INPUT_REQUIRED_VALUE_MISSING
    );
    assert!(output.diagnostics_were_truncated());
}
