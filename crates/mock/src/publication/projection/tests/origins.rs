use chigiri::core::{Object, Value};

use super::support::{capture, fixture, project};
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;
use crate::value::ValueBudget;

#[test]
fn dto_and_stored_origins_remain_non_record_data() {
    let fixture = fixture();
    let snapshot = Store::empty(fixture.ir.clone()).snapshot();
    let value = Value::Object(Object::from_iter([("value", Value::from("same"))]));
    let dto = snapshot.reader().output(fixture.dto, &value).unwrap();
    let stored = snapshot
        .reader()
        .stored_output(fixture.stored, &value)
        .unwrap();
    for (field, captured, expected_stored) in [
        (fixture.dto, capture(&fixture, fixture.dto, &dto), false),
        (
            fixture.stored,
            capture(&fixture, fixture.stored, &stored),
            true,
        ),
    ] {
        let projected = project(
            &captured,
            snapshot.clone(),
            field,
            &mut ValueBudget::default(),
        );
        let OutputValue::Object(object) = projected else {
            panic!("object required")
        };
        assert_eq!(
            matches!(object.data(), ObjectData::Stored(_)),
            expected_stored
        );
        assert_eq!(
            matches!(object.data(), ObjectData::Dto(_)),
            !expected_stored
        );
    }
}

#[test]
fn event_budget_failure_is_sticky_but_a_fresh_event_budget_recovers() {
    let fixture = fixture();
    let snapshot = Store::empty(fixture.ir.clone()).snapshot();
    let captured = capture(
        &fixture,
        fixture.text,
        &OutputValue::Scalar(Value::from("fresh")),
    );
    let mut exhausted = ValueBudget::default();
    exhausted.scratch(crate::api::WORK_LIMIT - 2).unwrap();
    assert!(
        captured.publications()[0]
            .project(snapshot.clone(), fixture.text, &mut exhausted)
            .is_err()
    );
    assert!(
        captured.publications()[0]
            .project(snapshot.clone(), fixture.text, &mut exhausted)
            .is_err()
    );
    assert!(matches!(
        project(
            &captured,
            snapshot,
            fixture.text,
            &mut ValueBudget::default()
        ),
        OutputValue::Scalar(Value::String(value)) if value == "fresh"
    ));
}

#[test]
fn json_array_scalar_is_not_reinterpreted_as_a_graphql_list() {
    let fixture = fixture();
    let snapshot = Store::empty(fixture.ir.clone()).snapshot();
    let captured = capture(
        &fixture,
        fixture.json,
        &OutputValue::Scalar(Value::List(vec![1.into(), 2.into()])),
    );
    assert!(matches!(
        project(
            &captured,
            snapshot,
            fixture.json,
            &mut ValueBudget::default()
        ),
        OutputValue::Scalar(Value::List(values)) if values == [Value::Int(1), Value::Int(2)]
    ));
}
