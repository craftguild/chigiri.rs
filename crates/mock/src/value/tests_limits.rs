//! Test repeated charges against one shared budget.

use chigiri::core::Value;

use super::tests::{ir, slot};
use super::{MAX_BYTES, MAX_DEPTH, MAX_NODES, ValueBudget, ValueValidator};

#[test]
fn rsm_16_value_depth_list_string_and_identity_boundaries_are_inclusive() {
    let ir = ir();
    let check = |name, value: &Value| {
        ValueValidator::new(&ir, &mut ValueBudget::default()).slot(slot(&ir, name), value)
    };
    let mut value = Value::Null;
    for _ in 1..MAX_DEPTH {
        value = Value::List(vec![value]);
    }
    assert!(check("json", &value).is_ok());
    value = Value::List(vec![value]);
    assert!(check("json", &value).is_err());
    assert!(check("json", &Value::List(vec![Value::Null; 1000])).is_ok());
    assert!(check("json", &Value::List(vec![Value::Null; 1001])).is_err());
    assert!(check("json", &Value::from("x".repeat(4096))).is_ok());
    assert!(check("json", &Value::from("x".repeat(4097))).is_err());
    for name in ["key", "peerKey"] {
        assert!(check(name, &Value::from("x".repeat(256))).is_ok());
        assert!(check(name, &Value::from("x".repeat(257))).is_err());
        assert!(check(name, &Value::from("")).is_err());
    }
    assert!(check("peerKey", &Value::Null).is_ok());
    assert!(check("key", &Value::Null).is_err());
}

#[test]
fn rsm_16_value_byte_work_is_shared_and_failure_is_sticky() {
    let ir = ir();
    let id = slot(&ir, "json");
    let value = Value::from("x".repeat(4096));
    let mut budget = ValueBudget::default();
    let error = (0..MAX_BYTES / 4096)
        .find_map(|_| ValueValidator::new(&ir, &mut budget).slot(id, &value).err())
        .expect("string-processing work must be exhausted");
    assert_eq!(error.0, "request aggregate limit");
    assert!(matches!(
        budget.work.failure(),
        Some(crate::limits::request::BudgetFailure::Hard(
            crate::limits::request::LimitNotice {
                kind: crate::limits::request::LimitKind::Work,
                ..
            }
        ))
    ));
    // A shared-work failure stops further byte charging while preserving the local 16 MiB ceiling.
    assert!(budget.bytes() > 4096 && budget.bytes() < MAX_BYTES);
    assert!(
        ValueValidator::new(&ir, &mut budget)
            .slot(id, &Value::Null)
            .is_err()
    );
    assert!(
        ValueValidator::new(&ir, &mut budget)
            .slot(id, &Value::from(""))
            .is_err()
    );
    assert!(
        ValueValidator::new(&ir, &mut ValueBudget::default())
            .slot(id, &value)
            .is_ok()
    );
}

#[test]
fn rsm_16_value_node_budget_counts_null_and_empty_containers() {
    let ir = ir();
    let id = slot(&ir, "json");
    let mut budget = ValueBudget::default();
    for _ in 0..MAX_NODES / 2 {
        ValueValidator::new(&ir, &mut budget)
            .slot(id, &Value::Null)
            .unwrap();
        ValueValidator::new(&ir, &mut budget)
            .slot(id, &Value::List(vec![]))
            .unwrap();
    }
    assert_eq!(budget.nodes(), MAX_NODES);
    assert_eq!(budget.bytes(), MAX_NODES);
    assert!(
        ValueValidator::new(&ir, &mut budget)
            .slot(id, &Value::Null)
            .is_err()
    );
}

#[test]
fn rsm_16_sparse_objects_use_required_field_plans_without_schema_width_scans() {
    use crate::ir::MockIr;
    use chigiri::core::{Object, Source};

    let fields = (0..2000)
        .map(|index| format!("f{index}:Int "))
        .collect::<String>();
    let source = format!(
        "input Input{{{fields}}} type Data{{{fields}}} type Item{{id:ID! data:Data}} type Query{{x(input:Input):Int}}"
    );
    let ir = MockIr::parse(vec![Source::new("wide.graphql", source)]).unwrap();
    let input_type = ir.graph().type_id("Input").unwrap();
    let data_type = ir.graph().type_id("Data").unwrap();
    assert_eq!(ir.graph().types()[input_type.0].inputs.len(), 2000);
    assert!(ir.graph().types()[input_type.0].required_inputs.is_empty());
    assert_eq!(ir.graph().types()[data_type.0].fields.len(), 2000);
    assert!(ir.required_value_fields(data_type).is_empty());
    let query = &ir.graph().types()[ir.graph().query().0];
    let input = ir.graph().fields()[query.fields["x"].0].arguments[0];
    let slot = slot(&ir, "data");
    let empty = Value::Object(Object::new());
    let mut budget = ValueBudget::default();
    for _ in 0..MAX_NODES / 2 {
        let mut validator = ValueValidator::new(&ir, &mut budget);
        validator.input(input, &empty).unwrap();
        validator.slot(slot, &empty).unwrap();
    }
    assert_eq!(budget.nodes(), MAX_NODES);
    assert_eq!(budget.bytes(), MAX_NODES);
}
