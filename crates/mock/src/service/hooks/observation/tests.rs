use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::{Source, Value};

use super::{Copy, object};
use crate::ir::MockIr;
use crate::output::OutputValue;
use crate::store::Store;
use crate::value::ValueBudget;

#[test]
fn hook_observation_copies_captured_fields_without_current_row_lookup() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "captured-observation.graphql",
            "type User { id: ID! name: String! } type Query { users: [User!]! }",
        )])
        .unwrap(),
    );
    let ty = ir.graph().type_id("User").unwrap();
    let model = ir.model_for(ty).unwrap();
    let slots = &ir.models()[model.0].slots;
    let values = BTreeMap::from([
        (slots["id"], Value::from("old")),
        (slots["name"], Value::from("captured")),
    ]);
    let snapshot = Store::empty(ir).snapshot();
    let OutputValue::Object(parent) =
        crate::output::captured_record(snapshot, ty, model, values, Some("old".into())).unwrap()
    else {
        panic!("object required")
    };
    let mut budget = ValueBudget::default();
    let Value::Object(observed) = object(&parent, &mut Copy(&mut budget), 1).unwrap() else {
        panic!("observed object required")
    };
    assert_eq!(
        observed.get("__typename").and_then(Value::as_str),
        Some("User")
    );
    assert_eq!(observed.get("id").and_then(Value::as_str), Some("old"));
    assert_eq!(
        observed.get("name").and_then(Value::as_str),
        Some("captured")
    );
}
