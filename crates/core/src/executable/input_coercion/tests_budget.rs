//! Verifies inclusive shared-budget boundaries and rejection of reuse after exhaustion.

use super::{
    CustomScalarRegistry, INPUT_RESOURCE_LIMIT, InputCoercionBudget, coerce_raw_with_budget,
};
use crate::language::parse_type_system_document;
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, SourceId, Value};

fn schema() -> SemanticSchema {
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(
        SourceId::new(0),
        "type Query { echo(value: [Int!]): Int }",
        &limits,
    );
    compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap()
}

#[test]
fn rsm_16_repeated_diagnostic_paths_share_bytes_before_rendering() {
    let key = "long".repeat(25);
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(
        SourceId::new(0),
        &format!("input I{{{key}:I xs:[Int]}} type Query{{value(arg:I):Int}}"),
        &limits,
    );
    let schema = compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap();
    let ty = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("value", "arg")
        .unwrap()
        .ty;
    let mut raw = Value::Object(
        [("xs", Value::List(vec![Value::from("bad"); 100]))]
            .into_iter()
            .collect(),
    );
    for _ in 0..8 {
        raw = Value::Object([(key.clone(), raw)].into_iter().collect());
    }
    let scalars = CustomScalarRegistry::new();
    // Generated diagnostics are maintained, and only a small shared byte budget of owned ones stops the increase.
    let unbounded = coerce_raw_with_budget(&schema, ty, raw.clone(), &scalars, &limits, None);
    assert_eq!(unbounded.diagnostics.len(), 100);
    let budget = InputCoercionBudget::new(1000, 4000);
    let output = coerce_raw_with_budget(&schema, ty, raw, &scalars, &limits, Some(&budget));
    assert!(!output.is_valid());
    assert_eq!(output.diagnostics.len(), 1);
    let error = output.diagnostics.iter().next().unwrap();
    assert_eq!(error.code, INPUT_RESOURCE_LIMIT);
    assert!(error.message.len() < 100);
    assert!(budget.exhausted());
    let fresh = InputCoercionBudget::new(1000, 4000);
    assert!(
        coerce_raw_with_budget(
            &schema,
            ty,
            Value::Object(Default::default()),
            &scalars,
            &limits,
            Some(&fresh)
        )
        .is_valid()
    );
}

#[test]
fn rsm_16_argument_names_are_reserved_before_path_and_public_key_copies() {
    use super::{CoercedVariables, coerce_argument_with_budget};
    let schema = schema();
    let definition = schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("echo", "value")
        .unwrap();
    let limits = ExecutionLimits::production();
    let variables = CoercedVariables::new();
    let scalars = CustomScalarRegistry::new();
    for bytes in [4, 5] {
        let budget = InputCoercionBudget::new(10, bytes);
        let result = coerce_argument_with_budget(
            &schema,
            definition,
            None,
            &variables,
            &scalars,
            &limits,
            Some(&budget),
        );
        assert_eq!(result.is_valid(), bytes == 5);
        let again = coerce_argument_with_budget(
            &schema,
            definition,
            None,
            &variables,
            &scalars,
            &limits,
            Some(&budget),
        );
        assert!(!again.is_valid());
    }
}

#[test]
fn rsm_16_coercion_nodes_and_bytes_are_inclusive_shared_and_sticky() {
    let schema = schema();
    let ty = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("echo", "value")
        .unwrap()
        .ty;
    let scalars = CustomScalarRegistry::new();
    let limits = ExecutionLimits::production();
    for (nodes, bytes, success) in [(3, 16, true), (2, 16, false), (3, 15, false)] {
        let budget = InputCoercionBudget::new(nodes, bytes);
        let input = Value::List(vec![Value::Int(1), Value::Int(2)]);
        let output = coerce_raw_with_budget(&schema, ty, input, &scalars, &limits, Some(&budget));
        assert_eq!(output.is_valid(), success);
        if !success {
            assert_eq!(
                output.diagnostics.iter().next().unwrap().code,
                INPUT_RESOURCE_LIMIT
            );
        }
        // A successful coercion must leave the exhausted budget unchanged.
        let output = coerce_raw_with_budget(
            &schema,
            ty,
            Value::List(vec![]),
            &scalars,
            &limits,
            Some(&budget),
        );
        assert!(!output.is_valid());
        assert_eq!(
            output.diagnostics.iter().next().unwrap().code,
            INPUT_RESOURCE_LIMIT
        );
    }
}

#[test]
fn rsm_16_diagnostic_zero_does_not_turn_resource_failure_into_a_value() {
    let schema = schema();
    let ty = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("echo", "value")
        .unwrap()
        .ty;
    let limits = ExecutionLimits {
        max_diagnostics: 0,
        ..ExecutionLimits::production()
    };
    let budget = InputCoercionBudget::new(0, 0);
    let output = coerce_raw_with_budget(
        &schema,
        ty,
        Value::Null,
        &CustomScalarRegistry::new(),
        &limits,
        Some(&budget),
    );
    assert!(!output.is_valid());
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn rsm_16_id_and_bigint_numeric_representations_reserve_their_full_bytes() {
    use super::{CoercedInput, CoercedVariables, coerce_literal_with_budget};
    use crate::Span;
    use crate::language::ast::{Name, TypeRef, Value as AstValue, ValueNode};
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(
        SourceId::new(0),
        "scalar Token type Query { q: Token }",
        &limits,
    );
    let schema = compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap();
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("Token", |_, value| Ok(value));
    let span = Span::new(SourceId::new(0), 0, 40);
    let raw = "9".repeat(256);
    let literal = ValueNode::new(AstValue::Int(raw.clone()), span);
    for (name, nodes, bytes) in [("ID", 1, raw.len()), ("Token", 2, raw.len() * 2)] {
        let ty = TypeRef::named(Name::new(name.to_owned(), span), span);
        for (bytes, valid) in [(bytes, true), (bytes - 1, false)] {
            let budget = InputCoercionBudget::new(nodes, bytes);
            let output = coerce_literal_with_budget(
                &schema,
                &ty,
                &literal,
                &CoercedVariables::new(),
                &scalars,
                &limits,
                Some(&budget),
            );
            assert_eq!(output.is_valid(), valid);
            if valid {
                assert_eq!(
                    output.value,
                    Some(CoercedInput::Value(if name == "ID" {
                        Value::String(raw.clone())
                    } else {
                        Value::BigInt(raw.clone())
                    }))
                );
            }
        }
    }
    let id = TypeRef::named(Name::new("ID".into(), span), span);
    let text = i128::MIN.to_string();
    for (bytes, valid) in [(text.len(), true), (text.len() - 1, false)] {
        let budget = InputCoercionBudget::new(1, bytes);
        let output = coerce_raw_with_budget(
            &schema,
            &id,
            Value::Int(i128::MIN),
            &scalars,
            &limits,
            Some(&budget),
        );
        assert_eq!(output.is_valid(), valid);
    }
}

#[test]
fn rsm_16_null_and_undefined_variables_each_charge_materialized_list_items() {
    use super::{CoercedInput, CoercedVariables, coerce_literal_with_budget};
    use crate::Span;
    use crate::language::ast::{Name, Value as AstValue, ValueNode};
    let limits = ExecutionLimits::production();
    let parsed =
        parse_type_system_document(SourceId::new(0), "type Query { q(v: [Int]): Int }", &limits);
    let schema = compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap();
    let ty = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("q", "v")
        .unwrap()
        .ty;
    let span = Span::new(SourceId::new(0), 0, 2);
    let item = ValueNode::new(AstValue::Variable(Name::new("n".into(), span)), span);
    let literal = ValueNode::new(AstValue::List(vec![item.clone(), item]), span);
    for value in [CoercedInput::Null, CoercedInput::Undefined] {
        let mut variables = CoercedVariables::new();
        variables.insert("n".into(), value);
        for (nodes, valid) in [(3, true), (2, false)] {
            let budget = InputCoercionBudget::new(nodes, 0);
            let output = coerce_literal_with_budget(
                &schema,
                ty,
                &literal,
                &variables,
                &CustomScalarRegistry::new(),
                &limits,
                Some(&budget),
            );
            assert_eq!(output.is_valid(), valid);
            if valid {
                assert_eq!(
                    output.value,
                    Some(CoercedInput::Value(Value::List(vec![
                        Value::Null,
                        Value::Null
                    ])))
                );
            }
        }
    }
}
