//! Runs constraints from SDL through the real input, default, and output
//! validators at the correct scope.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Source, Value};

use super::{ValueBudget, ValueValidator, conditions};
use crate::cancellation::Cancellation;
use crate::ir::{FieldId, MockIr, SchemaGraph};
use crate::store::Store;

fn ir(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("constraints.graphql", source)]).unwrap())
}

fn field(ir: &MockIr, name: &str) -> FieldId {
    ir.graph().types()[ir.graph().query().0].fields[name]
}

#[test]
fn sd_13_nested_lists_apply_items_once_and_leaf_constraints_recursively() {
    let ir = ir(r#"
        scalar Array @representation(type:ARRAY,minItems:1,maxItems:2)
        type Query {
          words(input:[[String]] @constraint(minItems:2,minLength:1)): [[String]] @constraint(minItems:2,minLength:1)
          arrays(input:[Array] @constraint(maxItems:1)): [Array] @constraint(maxItems:1)
        }
    "#);
    let snapshot = Store::empty(ir.clone()).snapshot();
    for (name, value, valid) in [
        (
            "words",
            Value::List(vec![
                Value::List(vec![Value::from("a")]),
                Value::List(vec![]),
            ]),
            true,
        ),
        (
            "words",
            Value::List(vec![Value::Null, Value::List(vec![Value::Null])]),
            true,
        ),
        ("words", Value::Null, true),
        (
            "words",
            Value::List(vec![
                Value::List(vec![Value::from("")]),
                Value::List(vec![]),
            ]),
            false,
        ),
        (
            "words",
            Value::List(vec![Value::List(vec![Value::from("a")])]),
            false,
        ),
        (
            "arrays",
            Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(2)])]),
            true,
        ),
        ("arrays", Value::List(vec![Value::List(vec![])]), false),
        (
            "arrays",
            Value::List(vec![Value::List(vec![Value::Null; 3])]),
            false,
        ),
    ] {
        let field = field(&ir, name);
        let input = ir.graph().fields()[field.0].arguments[0];
        assert_eq!(
            ValueValidator::new(&ir, &mut ValueBudget::default())
                .input(input, &value)
                .is_ok(),
            valid,
            "input {name}: {value:?}"
        );
        assert_eq!(
            crate::output::validate(&snapshot, field, &value, &mut ValueBudget::default()).is_ok(),
            valid,
            "output {name}: {value:?}"
        );
    }
}

#[test]
fn sd_13_unicode_scalar_counts_and_zero_are_not_truthiness_checks() {
    let ir = ir(r#"type Query {
        one(x:String @constraint(maxLength:1)):String @constraint(maxLength:1)
        empty(x:String @constraint(maxLength:0)):String @constraint(maxLength:0)
        text(x:String @constraint(startsWith:"A",endsWith:"Z",contains:"M",notContains:"X",minLength:3,maxLength:5)):String
    }"#);
    for (name, text, valid) in [
        ("one", "😀", true),
        ("one", "é", true),
        ("one", "e\u{301}", false),
        ("one", "✈\u{fe0f}", false),
        ("one", "👩\u{200d}💻", false),
        ("empty", "", true),
        ("empty", "x", false),
        ("text", "AMZ", true),
        ("text", "AMXZ", false),
        ("text", "AZ", false),
        ("text", "MZA", false),
    ] {
        let input = ir.graph().fields()[field(&ir, name).0].arguments[0];
        assert_eq!(
            ValueValidator::new(&ir, &mut ValueBudget::default())
                .input(input, &Value::from(text))
                .is_ok(),
            valid,
            "{name}: {text}"
        );
    }
}

#[test]
fn sd_13_defaults_and_runtime_input_output_share_numeric_and_format_checks() {
    for (ty, constraint, good, bad) in [
        (
            "Float",
            "min:0,max:1,exclusiveMin:0,exclusiveMax:1,multipleOf:0.1",
            "0.3",
            "0.31",
        ),
        (
            "String",
            "format:\"date\"",
            "\"2024-02-29\"",
            "\"2023-02-29\"",
        ),
        ("String", "pattern:\"^[a-z]+$\"", "\"abc\"", "\"123\""),
    ] {
        let make = |literal| {
            format!(
                "type Query{{x(arg:{ty} = {literal} @constraint({constraint})): {ty} @constraint({constraint})}}"
            )
        };
        let ir = ir(&make(good));
        assert!(MockIr::parse(vec![Source::new("bad.graphql", make(bad))]).is_err());
        let parse_value = |raw: &str| {
            if ty == "Float" {
                Value::Float(raw.parse().unwrap())
            } else {
                Value::from(raw.trim_matches('"'))
            }
        };
        let snapshot = Store::empty(ir.clone()).snapshot();
        let field = field(&ir, "x");
        for (literal, valid) in [(good, true), (bad, false)] {
            let value = parse_value(literal);
            assert_eq!(
                ValueValidator::new(&ir, &mut ValueBudget::default())
                    .input(ir.graph().fields()[field.0].arguments[0], &value)
                    .is_ok(),
                valid
            );
            assert_eq!(
                crate::output::validate(&snapshot, field, &value, &mut ValueBudget::default())
                    .is_ok(),
                valid
            );
        }
    }
}

#[test]
fn sd_14_regex_budget_and_cancellation_are_shared_and_sticky() {
    let ir = ir(r#"type Query{x:String @constraint(pattern:"") y:String @constraint(pattern:"")}"#);
    let x = &ir.graph().fields()[field(&ir, "x").0].constraints;
    let y = &ir.graph().fields()[field(&ir, "y").0].constraints;
    assert!(Arc::ptr_eq(
        x.pattern.as_ref().unwrap(),
        y.pattern.as_ref().unwrap()
    ));
    let mut first = ValueBudget::default();
    let mut second = ValueBudget::with_work(first.work.clone());
    for _ in 0..2048 {
        conditions::constraints(x, &Value::from(""), &mut first).unwrap();
        conditions::constraints(y, &Value::from(""), &mut second).unwrap();
    }
    assert!(conditions::constraints(x, &Value::from(""), &mut first).is_err());
    assert!(conditions::constraints(y, &Value::from(""), &mut second).is_err());
    let cancel = Cancellation::default();
    let shutdown = Cancellation::default();
    let mut budget = ValueBudget::default();
    budget
        .work
        .guard(&cancel, &shutdown, Instant::now() + Duration::from_secs(1));
    cancel.cancel();
    assert!(conditions::constraints(x, &Value::from(""), &mut budget).is_err());
    assert!(budget.work.check().is_err());
    let mut expired = ValueBudget::default();
    expired.work.guard(
        &Cancellation::default(),
        &shutdown,
        Instant::now() - Duration::from_millis(1),
    );
    assert!(conditions::constraints(x, &Value::from(""), &mut expired).is_err());
}

#[test]
fn sd_12_static_numeric_intersections_and_closed_format_names() {
    for constraint in [
        "multipleOf:0",
        "multipleOf:-1",
        "multipleOf:1e-999",
        "multipleOf:1e999",
        "min:0,exclusiveMax:0",
        "min:1,max:2,multipleOf:0.3",
    ] {
        assert!(
            SchemaGraph::parse(vec![Source::new(
                "bad",
                format!("type Query{{x:Int @constraint({constraint})}}")
            )])
            .is_err(),
            "{constraint}"
        );
    }
    for name in ["url", "Date", "emailAddress", "DATE-TIME"] {
        assert!(
            SchemaGraph::parse(vec![Source::new(
                "bad",
                format!("type Query{{x:String @constraint(format:\"{name}\")}}")
            )])
            .is_err()
        );
    }
}
