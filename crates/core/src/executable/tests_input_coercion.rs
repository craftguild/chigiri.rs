use super::*;
use crate::language::ast::{Definition, ExecutableDefinition, OperationDefinition};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, Object, SourceId, Value};

fn schema() -> SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(31),
        r#"
        scalar Token
        enum Color { RED GREEN }
        input Config {
          required: Int!
          optional: String
          limit: Int = 5
          colors: [Color!]!
        }
        input Choice @oneOf { id: ID, name: String }
        type Query {
          run(config: Config, choice: Choice, token: Token, count: Int = 7): Boolean
        }
        "#,
        &ExecutionLimits::unbounded(),
    );
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let output = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.schema.unwrap()
}

fn operation(source: &str) -> OperationDefinition {
    let parsed =
        parse_executable_document(SourceId::new(32), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        parsed.document.unwrap().definitions.remove(0)
    else {
        panic!("expected an operation.");
    };
    operation
}

fn argument<'a>(
    schema: &'a SemanticSchema,
    name: &str,
) -> &'a crate::language::ast::InputValueDefinition {
    schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("run", name)
        .unwrap()
}

fn codes(output: &InputCoercionOutput) -> Vec<crate::DiagnosticCode> {
    output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn coerces_defaults_singleton_lists_and_enums() {
    let schema = schema();
    let definition = argument(&schema, "config");
    let mut raw = Object::new();
    raw.insert("required", 3_i32);
    raw.insert("colors", "RED");

    let output = coerce_raw_input(
        &schema,
        &definition.ty,
        Value::Object(raw),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    let Some(CoercedInput::Value(Value::Object(value))) = output.value else {
        panic!(
            "expected input coercion to produce an object: {:?}",
            output.diagnostics.into_vec()
        );
    };
    assert_eq!(value.get("required"), Some(&Value::Int(3)));
    assert_eq!(value.get("limit"), Some(&Value::Int(5)));
    assert_eq!(
        value.get("colors"),
        Some(&Value::List(vec![Value::String("RED".to_owned())]))
    );
    assert!(!value.contains_key("optional"));
}

#[test]
fn diagnoses_unknown_required_and_one_of_input_fields() {
    let schema = schema();
    let mut invalid = Object::new();
    invalid.insert("unknown", true);
    let output = coerce_raw_input(
        &schema,
        &argument(&schema, "config").ty,
        Value::Object(invalid),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    let found = codes(&output);
    assert_eq!(
        found,
        vec![
            INPUT_UNKNOWN_FIELD,
            INPUT_REQUIRED_VALUE_MISSING,
            INPUT_REQUIRED_VALUE_MISSING,
        ]
    );

    let mut choice = Object::new();
    choice.insert("id", Value::Null);
    let output = coerce_raw_input(
        &schema,
        &argument(&schema, "choice").ty,
        Value::Object(choice),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(codes(&output), vec![INPUT_ONE_OF_VALUE]);

    let mut choice = Object::new();
    choice.insert("id", 42_i32);
    let output = coerce_raw_input(
        &schema,
        &argument(&schema, "choice").ty,
        Value::Object(choice),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    let Some(CoercedInput::Value(Value::Object(choice))) = output.value else {
        panic!("expected valid OneOf coercion");
    };
    assert_eq!(choice.get("id"), Some(&Value::String("42".to_owned())));
}

#[test]
fn distinguishes_missing_null_and_defaulted_variables_from_singleton_lists() {
    let schema = schema();
    let operation = operation(
        "query Q($required: Int!, $defaulted: Int = 4, $nullable: String, $ids: [ID!]!) { __typename }",
    );
    let mut raw = Object::new();
    raw.insert("required", 9_i32);
    raw.insert("ids", 12_i32);
    let output = coerce_variable_values(
        &schema,
        &operation.variable_definitions,
        raw,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    let variables = output.variables.unwrap();
    assert_eq!(
        variables.get("required"),
        Some(&CoercedInput::Value(Value::Int(9)))
    );
    assert_eq!(
        variables.get("defaulted"),
        Some(&CoercedInput::Value(Value::Int(4)))
    );
    assert_eq!(variables.get("nullable"), Some(&CoercedInput::Undefined));
    assert_eq!(
        variables.get("ids"),
        Some(&CoercedInput::Value(Value::List(vec![Value::String(
            "12".to_owned()
        )])))
    );

    let missing = coerce_variable_values(
        &schema,
        &operation.variable_definitions,
        Object::new(),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert!(missing.variables.is_none());
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == INPUT_REQUIRED_VALUE_MISSING)
    );

    let mut explicit_null = Object::new();
    explicit_null.insert("required", Value::Null);
    explicit_null.insert("ids", Value::List(Vec::new()));
    let explicit_null = coerce_variable_values(
        &schema,
        &operation.variable_definitions,
        explicit_null,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert!(
        explicit_null
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == INPUT_NULL_FOR_NON_NULL)
    );
}

#[test]
fn coerces_ast_literals_to_builtin_types() {
    let schema = schema();
    let operation = operation("query Q { run(count: 6) }");
    let crate::language::ast::Selection::Field(field) = &operation.selection_set.selections[0]
    else {
        panic!("expected a field.");
    };
    let output = coerce_literal_input(
        &schema,
        &argument(&schema, "count").ty,
        &field.arguments[0].value,
        &CoercedVariables::new(),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(output.value, Some(CoercedInput::Value(Value::Int(6))));
}

#[test]
fn undefined_variables_use_argument_defaults() {
    let schema = schema();
    let operation = operation("query Q($count: Int) { run(count: $count) }");
    let variables = coerce_variable_values(
        &schema,
        &operation.variable_definitions,
        Object::new(),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    )
    .variables
    .unwrap();
    let crate::language::ast::Selection::Field(field) = &operation.selection_set.selections[0]
    else {
        panic!("expected a field.");
    };
    let output = coerce_argument_value(
        &schema,
        argument(&schema, "count"),
        Some(&field.arguments[0].value),
        &variables,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(output.value, Some(CoercedInput::Value(Value::Int(7))));
}

#[test]
fn custom_scalar_callbacks_receive_the_name_and_normalized_value() {
    let schema = schema();
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("Token", |name, value| match (name, value) {
        ("Token", Value::String(value)) if value.starts_with("tok_") => Ok(Value::String(value)),
        _ => Err(ScalarCoercionError::public("Token must start with `tok_`")),
    });
    let expected = &argument(&schema, "token").ty;
    let valid = coerce_raw_input(
        &schema,
        expected,
        Value::String("tok_ok".to_owned()),
        &scalars,
        &ExecutionLimits::unbounded(),
    );
    assert!(valid.is_valid());
    let invalid = coerce_raw_input(
        &schema,
        expected,
        Value::String("bad".to_owned()),
        &scalars,
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(codes(&invalid), vec![INPUT_CUSTOM_SCALAR_VALUE]);
    assert!(
        invalid
            .diagnostics
            .iter()
            .next()
            .unwrap()
            .message
            .contains("tok_")
    );
}
