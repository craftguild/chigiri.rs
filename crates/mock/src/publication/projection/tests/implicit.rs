use chigiri::core::{Object, Value};

use super::support::{
    capture, captured_record, fixture, project, record_output, seeded, selection, slot,
};
use crate::output::{ObjectData, OutputValue};
use crate::store::CallScope;
use crate::value::ValueBudget;

#[test]
fn implicit_edges_use_fresh_membership_and_identity_reuse_without_rebinding_root_fields() {
    let fixture = fixture();
    let mut store = seeded(&fixture);
    let old = store.snapshot();
    let a = capture(
        &fixture,
        fixture.a_event,
        &record_output(&old, fixture.a_event, fixture.a, 0),
    );
    let b = capture(
        &fixture,
        fixture.b_event,
        &record_output(&old, fixture.b_event, fixture.b, 0),
    );

    let mut connect = store.begin(CallScope::MutationRoot).unwrap();
    connect
        .update(
            fixture.a,
            &selection(&[("id", 1.into())]),
            &Object::from_iter([("name", Value::from("fresh-a"))]),
        )
        .unwrap();
    connect
        .update(
            fixture.b,
            &selection(&[("id", true.into())]),
            &Object::from_iter([("label", Value::from("fresh-b"))]),
        )
        .unwrap();
    connect
        .connect(
            fixture.a,
            &selection(&[("id", 1.into())]),
            "bs",
            &selection(&[("id", true.into())]),
        )
        .unwrap();
    let fresh = store.commit(connect.prepare().unwrap()).unwrap();

    let projected_a = project(
        &a,
        fresh.clone(),
        fixture.a_event,
        &mut ValueBudget::default(),
    );
    assert_eq!(
        captured_record(&projected_a)
            .get(slot(&fixture, fixture.a, "name"))
            .and_then(Value::as_str),
        Some("captured-a")
    );
    let bs = relation(&fixture, &fresh, &projected_a, fixture.a, "bs");
    assert_current(&fixture, &bs, fixture.b, "label", "fresh-b");

    let projected_b = project(
        &b,
        fresh.clone(),
        fixture.b_event,
        &mut ValueBudget::default(),
    );
    let as_ = relation(&fixture, &fresh, &projected_b, fixture.b, "as");
    assert_current(&fixture, &as_, fixture.a, "name", "fresh-a");

    let mut recreate = store.begin(CallScope::MutationRoot).unwrap();
    recreate
        .delete(fixture.a, &selection(&[("id", 1.into())]))
        .unwrap();
    recreate
        .create(
            fixture.a,
            &Object::from_iter([
                ("id", Value::from(1)),
                ("name", Value::from("replacement-a")),
            ]),
        )
        .unwrap();
    recreate
        .connect(
            fixture.a,
            &selection(&[("id", 1.into())]),
            "bs",
            &selection(&[("id", true.into())]),
        )
        .unwrap();
    let recreated = store.commit(recreate.prepare().unwrap()).unwrap();
    let projected = project(
        &a,
        recreated.clone(),
        fixture.a_event,
        &mut ValueBudget::default(),
    );
    let bs = relation(&fixture, &recreated, &projected, fixture.a, "bs");
    assert_current(&fixture, &bs, fixture.b, "label", "fresh-b");
}

fn relation(
    fixture: &super::support::Fixture,
    snapshot: &crate::store::Snapshot,
    parent: &OutputValue,
    model: crate::ir::ModelId,
    name: &str,
) -> OutputValue {
    let field = fixture.ir.graph().types()[fixture.ir.models()[model.0].ty.0].fields[name];
    snapshot
        .reader()
        .captured_related_output(field, captured_record(parent))
        .unwrap()
}

fn assert_current(
    fixture: &super::support::Fixture,
    output: &OutputValue,
    model: crate::ir::ModelId,
    field: &str,
    expected: &str,
) {
    let OutputValue::List(values) = output else {
        panic!("relation list required")
    };
    let OutputValue::Object(object) = &values[0] else {
        panic!("related object required")
    };
    let ObjectData::Model(record) = object.data() else {
        panic!("fresh model required")
    };
    assert_eq!(
        record
            .get(slot(fixture, model, field))
            .and_then(Value::as_str),
        Some(expected)
    );
}
