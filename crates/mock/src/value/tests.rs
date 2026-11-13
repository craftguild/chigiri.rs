//! Verifies values through the shared validator.

use chigiri::core::{Object, Source, Value};

use super::{ValueBudget, ValueValidator, exact_number};
use crate::ir::{InputId, MockIr, SlotId};

pub(super) const SDL: &str = r#"
scalar Json @representation(type:JSON)
scalar Integer @representation(type:INTEGER)
scalar Number @representation(type:NUMBER,min:-10,max:10)
scalar Text @representation(type:STRING,minBytes:2,maxBytes:4)
scalar Truth @representation(type:BOOLEAN)
scalar Map @representation(type:OBJECT)
scalar Array @representation(type:ARRAY,minItems:1,maxItems:3)
enum Flag { YES NO }
type Dto { text:String! count:Int next:Dto computed:Int @virtual }
type Item {
  key:String! title:String @constraint(minLength:2,maxLength:2)
  json:Json integer:Integer number:Number text:Text truth:Truth map:Map array:Array
  numbers:[Integer]! @constraint(minItems:1,maxItems:2)
  matrix:[[Int!]!]! flag:Flag dto:Dto
  peer:Item @relation(fields:["peerKey"],references:["key"])
}
input Choice @oneOf { text:String count:Int }
input Input {
  n:Int! defaulted:String = "ok" next:Input choice:Choice
  text:String @constraint(minLength:2,maxLength:2)
}
type Query { x(input:Input):Int }
"#;

pub(super) fn ir() -> MockIr {
    MockIr::parse(vec![Source::new("values.graphql", SDL)]).unwrap()
}

pub(super) fn slot(ir: &MockIr, name: &str) -> SlotId {
    *ir.models()[ir.model_for(ir.graph().type_id("Item").unwrap()).unwrap().0]
        .slots
        .get(name)
        .unwrap()
}

pub(super) fn object(values: &[(&str, Value)]) -> Value {
    Value::Object(values.iter().cloned().collect::<Object>())
}

fn check(ir: &MockIr, name: &str, value: &Value) -> bool {
    ValueValidator::new(ir, &mut ValueBudget::default())
        .slot(slot(ir, name), value)
        .is_ok()
}

#[test]
fn rsm_04_scalar_shapes_and_constraints_apply_to_values() {
    let ir = ir();
    for (name, value) in [
        ("title", Value::from("ok")),
        ("text", Value::from("sun")),
        ("truth", Value::Boolean(true)),
        ("integer", Value::Float(2.0)),
        ("number", Value::Float(-10.0)),
        ("map", object(&[])),
        ("array", Value::List(vec![Value::Null])),
        ("flag", Value::from("YES")),
        ("json", Value::List(vec![])),
        ("json", object(&[("key", Value::Null)])),
        ("json", Value::Boolean(false)),
        ("text", Value::Null),
    ] {
        assert!(check(&ir, name, &value), "{name}");
    }
    for (name, value) in [
        ("title", Value::from("too long")),
        ("text", Value::from("x")),
        ("truth", Value::from("true")),
        ("integer", Value::Float(2.5)),
        ("number", Value::Int(11)),
        ("map", Value::List(vec![])),
        ("array", object(&[])),
        ("array", Value::List(vec![])),
        ("flag", Value::from("yes")),
        ("json", object(&[("key", Value::Float(f64::INFINITY))])),
    ] {
        assert!(!check(&ir, name, &value), "{name}");
    }
}

#[test]
fn rep_05_datetime_name_and_specified_by_do_not_replace_field_format() {
    let source = r#"
        scalar DateTime @specifiedBy(url:"https://invalid.invalid/codec")
            @representation(type:STRING,minBytes:2)
        type Item { id:ID! plain:DateTime formatted:DateTime @constraint(format:"date-time") }
        type Query { items:[Item!]! }
    "#;
    let ir = MockIr::parse(vec![Source::new("datetime.graphql", source)]).unwrap();
    let invalid_date = Value::from("not-a-date");
    assert!(check(&ir, "plain", &invalid_date));
    assert!(!check(&ir, "formatted", &invalid_date));
    assert!(check(
        &ir,
        "formatted",
        &Value::from("2024-01-01T00:00:00Z")
    ));
    assert!(!check(&ir, "formatted", &Value::from("x")));
}

#[test]
fn rsm_09_numbers_preserve_exact_large_integers_and_reject_rounding() {
    for value in [
        Value::Int(i128::MIN),
        Value::Int(9_007_199_254_740_992),
        Value::Int(9_007_199_254_740_994),
        Value::BigInt("1606938044258990275541962092341162602522202993782792835301376".into()),
        Value::Float(-0.0),
        Value::Float(f64::MAX),
    ] {
        assert!(exact_number(&value).is_ok());
    }
    for value in [
        Value::Int(i128::MAX),
        Value::Int(9_007_199_254_740_993),
        Value::BigInt("1606938044258990275541962092341162602522202993782792835301377".into()),
        Value::BigInt("1".into()),
        Value::BigInt("1e200".into()),
        Value::BigInt("9".repeat(311)),
        Value::Float(f64::NAN),
        Value::Float(f64::NEG_INFINITY),
    ] {
        assert!(exact_number(&value).is_err());
    }
    let ir = ir();
    let list = |number| Value::List(vec![Value::List(vec![Value::Int(number)])]);
    assert!(check(&ir, "matrix", &list(i128::from(i32::MIN))));
    assert!(check(&ir, "matrix", &list(i128::from(i32::MAX))));
    assert!(!check(&ir, "matrix", &list(i128::from(i32::MAX) + 1)));
    assert!(!check(&ir, "matrix", &list(i128::from(i32::MIN) - 1)));
}

#[test]
fn rsm_14_lists_keep_outer_constraints_and_item_nullability() {
    let ir = ir();
    assert!(check(&ir, "numbers", &Value::List(vec![Value::Null])));
    assert!(check(
        &ir,
        "numbers",
        &Value::List(vec![Value::Int(3), Value::Null])
    ));
    for value in [
        Value::Null,
        Value::List(vec![]),
        Value::List(vec![Value::Null; 3]),
    ] {
        assert!(!check(&ir, "numbers", &value));
    }
    assert!(check(
        &ir,
        "matrix",
        &Value::List(vec![Value::List(vec![])])
    ));
    assert!(!check(&ir, "matrix", &Value::List(vec![Value::Null])));
    assert!(!check(
        &ir,
        "matrix",
        &Value::List(vec![Value::List(vec![Value::Null])])
    ));
}

#[test]
fn rsm_14_stored_objects_reject_computed_unknown_missing_and_validate_embedded_models() {
    let ir = ir();
    let dto = object(&[("text", Value::from("ok")), ("next", Value::Null)]);
    assert!(check(&ir, "dto", &dto));
    for value in [
        object(&[]),
        object(&[("text", Value::Null)]),
        object(&[("text", Value::from("ok")), ("computed", Value::Int(1))]),
        object(&[("text", Value::from("ok")), ("unknown", Value::Null)]),
    ] {
        assert!(!check(&ir, "dto", &value));
    }
    let source = SDL.replace("count:Int next:Dto", "count:Int model:Item next:Dto");
    let ir = MockIr::parse(vec![Source::new("inline.graphql", source)]).unwrap();
    assert!(!check(
        &ir,
        "dto",
        &object(&[
            ("text", Value::from("ok")),
            ("model", object(&[("key", Value::from("Item:1"))])),
        ])
    ));
    assert!(check(
        &ir,
        "dto",
        &object(&[
            ("text", "ok".into()),
            (
                "model",
                object(&[
                    ("key", "embedded".into()),
                    ("numbers", Value::List(vec![Value::Null])),
                    ("matrix", Value::List(vec![])),
                ])
            ),
        ])
    ));
}

#[test]
fn rsm_14_coerced_input_distinguishes_missing_null_defaults_and_oneof() {
    let ir = ir();
    let query = &ir.graph().types()[ir.graph().query().0];
    let input: InputId = ir.graph().fields()[query.fields["x"].0].arguments[0];
    let check =
        |value: &Value| ValueValidator::new(&ir, &mut ValueBudget::default()).input(input, value);
    let base = [("n", Value::Int(1)), ("defaulted", Value::from("ok"))];
    assert!(check(&object(&base)).is_ok());
    let mut with_null = base.to_vec();
    with_null.push(("next", Value::Null));
    assert!(check(&object(&with_null)).is_ok());
    for value in [
        object(&[]),
        object(&[("n", Value::Int(1))]),
        object(&[("n", Value::Null), ("defaulted", Value::from("ok"))]),
    ] {
        assert!(check(&value).is_err());
    }
    for choice in [
        object(&[]),
        object(&[("text", Value::Null)]),
        object(&[("text", Value::from("x")), ("count", Value::Int(1))]),
    ] {
        let mut values = base.to_vec();
        values.push(("choice", choice));
        assert!(check(&object(&values)).is_err());
    }
    let mut values = base.to_vec();
    values.push(("choice", object(&[("text", Value::from("x"))])));
    assert!(check(&object(&values)).is_ok());
}
