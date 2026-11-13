//! Test raw-input coercion boundaries for built-in scalars.

use super::{
    CoercedInput, CoercedVariables, CustomScalarRegistry, INPUT_SCALAR_VALUE, coerce_literal_input,
    coerce_raw_input,
};
use crate::language::ast::{
    Definition, ExecutableDefinition, TypeRef, Value as AstValue, ValueNode,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, SourceId, Span, Value};

fn schema() -> SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(51),
        "type Query { value: String }",
        &ExecutionLimits::unbounded(),
    );
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

fn variable_type(source: &str) -> TypeRef {
    let parsed =
        parse_executable_document(SourceId::new(52), source, &ExecutionLimits::unbounded());
    let document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    operation.variable_definitions[0].ty.clone()
}

fn coerce(expected: &TypeRef, value: Value) -> super::InputCoercionOutput {
    coerce_raw_input(
        &schema(),
        expected,
        value,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    )
}

#[test]
fn applies_builtin_scalar_boundaries_and_canonical_id_conversion() {
    let int = variable_type("query Q($value: Int) { __typename }");
    let valid = coerce(&int, Value::Int(i128::from(i32::MIN)));
    assert_eq!(
        valid.value,
        Some(CoercedInput::Value(Value::Int(i128::from(i32::MIN))))
    );
    let invalid = coerce(&int, Value::Int(i128::from(i32::MAX) + 1));
    assert_eq!(
        invalid.diagnostics.iter().next().unwrap().code,
        INPUT_SCALAR_VALUE
    );
    assert!(!coerce(&int, Value::BigInt("2147483648".to_owned())).is_valid());

    let float = variable_type("query Q($value: Float) { __typename }");
    assert_eq!(
        coerce(&float, Value::Int(3)).value,
        Some(CoercedInput::Value(Value::Float(3.0)))
    );
    assert!(coerce(&float, Value::Int(9_007_199_254_740_992)).is_valid());
    assert!(!coerce(&float, Value::Int(9_007_199_254_740_993)).is_valid());
    let two_to_200 = "1606938044258990275541962092341162602522202993782792835301376";
    let two_to_200_plus_one = "1606938044258990275541962092341162602522202993782792835301377";
    assert!(coerce(&float, Value::BigInt(two_to_200.to_owned())).is_valid());
    assert!(!coerce(&float, Value::BigInt(two_to_200_plus_one.to_owned())).is_valid());
    assert!(!coerce(&float, Value::Float(f64::INFINITY)).is_valid());

    let string = variable_type("query Q($value: String) { __typename }");
    assert_eq!(
        coerce(&string, Value::String("text".to_owned())).value,
        Some(CoercedInput::Value(Value::String("text".to_owned())))
    );
    assert!(!coerce(&string, Value::Boolean(true)).is_valid());

    let boolean = variable_type("query Q($value: Boolean) { __typename }");
    assert_eq!(
        coerce(&boolean, Value::Boolean(false)).value,
        Some(CoercedInput::Value(Value::Boolean(false)))
    );
    assert!(!coerce(&boolean, Value::String("false".to_owned())).is_valid());

    let id = variable_type("query Q($value: ID) { __typename }");
    assert_eq!(
        coerce(&id, Value::Int(-42)).value,
        Some(CoercedInput::Value(Value::String("-42".to_owned())))
    );
    assert_eq!(
        coerce(&id, Value::BigInt(two_to_200.to_owned())).value,
        Some(CoercedInput::Value(Value::String(two_to_200.to_owned())))
    );

    let raw = "170141183460469231731687303715884105728";
    let literal = ValueNode::new(
        AstValue::Int(raw.to_owned()),
        Span::new(SourceId::new(53), 0, raw.len()),
    );
    let output = coerce_literal_input(
        &schema(),
        &id,
        &literal,
        &CoercedVariables::new(),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(
        output.value,
        Some(CoercedInput::Value(Value::String(raw.to_owned())))
    );
}

#[test]
fn rsm_04_owned_custom_decimal_checks_integer_precision_and_keeps_generated_float_contract() {
    use super::{InputCoercionBudget, coerce_literal_with_budget};
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(
        SourceId::new(0),
        "scalar N type Query{value(arg:N):Float}",
        &limits,
    );
    let schema = compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap();
    let custom = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("value", "arg")
        .unwrap()
        .ty;
    let float = variable_type("query($x:Float){__typename}");
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("N", |_, value| Ok(value));
    for (raw, valid) in [
        ("42.0", true),
        ("4.20e1", true),
        ("0.001e3", true),
        ("-0.0", true),
        ("1.25", true),
        ("1.0000000000000001", true),
        ("0.99999999999999999", true),
        ("1e-300", true),
        ("9007199254740994e0", true),
        ("9007199254740993.0", false),
        ("-9007199254740993e0", false),
        ("1e308", false),
        (
            "1606938044258990275541962092341162602522202993782792835301376.0",
            true,
        ),
    ] {
        let literal = ValueNode::new(
            AstValue::Float(raw.into()),
            Span::new(SourceId::new(0), 0, raw.len()),
        );
        let variables = CoercedVariables::new();
        let budget = InputCoercionBudget::new(1000, 4000);
        let output = coerce_literal_with_budget(
            &schema,
            custom,
            &literal,
            &variables,
            &scalars,
            &limits,
            Some(&budget),
        );
        assert_eq!(output.is_valid(), valid, "{raw}");
        // The custom scalar generated and the built-in Float do not change the existing limited f64 logic.
        assert!(
            coerce_literal_input(&schema, custom, &literal, &variables, &scalars, &limits)
                .is_valid(),
            "{raw}"
        );
        assert!(
            coerce_literal_with_budget(
                &schema,
                &float,
                &literal,
                &variables,
                &scalars,
                &limits,
                Some(&budget)
            )
            .is_valid(),
            "{raw}"
        );
    }
}
