//! Test owner edits, inverse ordering, and no-ops, including stored-slot and GraphQL-name differences.

use crate::store::test_selection::parent as parent_record;

use crate::store::test_selection::id as select;

use super::tests::{field, ir, model, object};
use super::{CallScope, Related, Store};
use crate::ir::ModelId;
use chigiri::core::{Object, Value};
use std::sync::Arc;

pub(super) const NETWORK: &str = r#"
type Person {
  id:ID!
  partner:Person @relation(fields:["partnerId"],references:["id"])
  paired:Person @relation
  groups:[Group]! @relation(fields:["groupIds"],references:["id"],name:"groups")
  optional:[Group] @relation(fields:["optionalIds"],references:["id"])
  required:Group! @relation(fields:["requiredId"],references:["id"])
}
type Group { id:ID! members:[Person!]! @relation(name:"groups") }
type Query { x:Int }
"#;

pub(super) fn network() -> (Store, ModelId, ModelId) {
    let ir = ir(NETWORK);
    let (person, group) = (model(&ir, "Person"), model(&ir, "Group"));
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    for id in 1..=3 {
        tx.create(group, &object(&[("id", format!("Group-{id}").into())]))
            .unwrap();
    }
    for (id, groups) in [
        (
            "Person-1",
            Value::List(vec!["Group-2".into(), Value::Null, "Group-1".into()]),
        ),
        ("Person-2", Value::List(vec![])),
    ] {
        tx.create(
            person,
            &object(&[
                ("id", id.into()),
                ("groupIds", groups),
                ("optionalIds", Value::Null),
                ("requiredId", "Group-1".into()),
            ]),
        )
        .unwrap();
    }
    store.commit(tx.prepare().unwrap()).unwrap();
    (store, person, group)
}

#[test]
fn mr_10_11_list_edit_preserves_null_order_snapshot_and_inverse_and_noops() {
    let (mut store, person, _) = network();
    let old = store.snapshot();
    let ir = old.ir();
    let slot = ir.models()[person.0].slots["groupIds"];
    let original = old.record(person, "Person-1").unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let unchanged = tx
        .connect(person, &select("Person-1"), "groups", &select("Group-2"))
        .unwrap()
        .remove(0);
    assert!(Arc::ptr_eq(&unchanged, original));
    let unchanged = tx
        .disconnect(person, &select("Person-1"), "groups", &select("Group-3"))
        .unwrap()
        .remove(0);
    assert!(Arc::ptr_eq(&unchanged, original));
    let added = tx
        .connect(person, &select("Person-1"), "groups", &select("Group-3"))
        .unwrap()
        .remove(0);
    assert_eq!(
        added.get(slot),
        Some(&Value::List(vec![
            "Group-2".into(),
            Value::Null,
            "Group-1".into(),
            "Group-3".into()
        ]))
    );
    let removed = tx
        .disconnect(person, &select("Person-1"), "groups", &select("Group-2"))
        .unwrap()
        .remove(0);
    assert_eq!(
        removed.get(slot),
        Some(&Value::List(vec![
            Value::Null,
            "Group-1".into(),
            "Group-3".into()
        ]))
    );
    let created = tx
        .create(
            person,
            &object(&[
                ("id", "Person-3".into()),
                ("groupIds", Value::List(vec![])),
                ("requiredId", "Group-1".into()),
            ]),
        )
        .unwrap();
    assert_eq!(created.id().unwrap(), "Person-3");
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(
        original.get(slot),
        Some(&Value::List(vec![
            "Group-2".into(),
            Value::Null,
            "Group-1".into()
        ]))
    );
    let ids = store.snapshot().find(person, &Object::new()).unwrap();
    assert_eq!(
        ids.iter()
            .map(|record| record.id().unwrap())
            .collect::<Vec<_>>(),
        ["Person-1", "Person-2", "Person-3"]
    );
    for (snapshot, target, expected) in [
        (&old, "Group-3", 0),
        (&store.snapshot(), "Group-3", 1),
        (&store.snapshot(), "Group-2", 0),
    ] {
        let Related::Many(owners) = snapshot
            .related(
                field(ir, "Group", "members"),
                &parent_record(snapshot, field(ir, "Group", "members"), target),
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(owners.len(), expected);
    }
}

#[test]
fn mr_10_11_nullable_list_distinguishes_null_empty_and_remaining_null_items() {
    let (store, person, _) = network();
    let slot = store.snapshot().ir().models()[person.0].slots["optionalIds"];
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let unchanged = tx
        .disconnect(person, &select("Person-1"), "optional", &select("absent"))
        .unwrap()
        .remove(0);
    assert_eq!(unchanged.get(slot), Some(&Value::Null));
    let connected = tx
        .connect(person, &select("Person-1"), "optional", &select("Group-3"))
        .unwrap()
        .remove(0);
    assert_eq!(
        connected.get(slot),
        Some(&Value::List(vec!["Group-3".into()]))
    );
    let empty = tx
        .disconnect(person, &select("Person-1"), "optional", &select("Group-3"))
        .unwrap()
        .remove(0);
    assert_eq!(empty.get(slot), Some(&Value::List(vec![])));
    tx.update(
        person,
        &select("Person-1"),
        &object(&[(
            "optionalIds",
            Value::List(vec![Value::Null, "Group-3".into(), Value::Null]),
        )]),
    )
    .unwrap();
    let empty = tx
        .disconnect(person, &select("Person-1"), "optional", &select("Group-3"))
        .unwrap()
        .remove(0);
    assert_eq!(
        empty.get(slot),
        Some(&Value::List(vec![Value::Null, Value::Null]))
    );
    tx.prepare().unwrap();
}

#[test]
fn mr_10_11_12_singular_self_connection_noop_replacement_and_required_disconnect() {
    let (mut store, person, _) = network();
    let slot = store.snapshot().ir().models()[person.0].slots["partnerId"];
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let connected = tx
        .connect(person, &select("Person-1"), "partner", &select("Person-1"))
        .unwrap()
        .remove(0);
    assert_eq!(connected.get(slot), Some(&Value::from("Person-1")));
    assert!(Arc::ptr_eq(
        &connected,
        &tx.connect(person, &select("Person-1"), "partner", &select("Person-1"))
            .unwrap()
            .remove(0)
    ));
    assert!(Arc::ptr_eq(
        &connected,
        &tx.disconnect(person, &select("Person-1"), "partner", &select("missing"))
            .unwrap()
            .remove(0)
    ));
    store.commit(tx.prepare().unwrap()).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.connect(person, &select("Person-1"), "partner", &select("Person-2"))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let cleared = tx
        .disconnect(person, &select("Person-1"), "partner", &select("Person-1"))
        .unwrap()
        .remove(0);
    assert_eq!(cleared.get(slot), Some(&Value::Null));
    tx.prepare().unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.disconnect(person, &select("Person-1"), "required", &select("Group-2"))
        .unwrap()
        .remove(0);
    assert!(
        tx.disconnect(person, &select("Person-1"), "required", &select("Group-1"))
            .is_err()
    );
    assert!(tx.prepare().is_err());
}

#[test]
fn mr_11_zero_target_selection_does_not_remove_unresolved_candidate_edges() {
    let (store, person, _) = network();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(
        person,
        &select("Person-1"),
        &object(&[
            ("partnerId", "pending".into()),
            ("optionalIds", Value::List(vec!["pending".into()])),
        ]),
    )
    .unwrap();
    tx.disconnect(person, &select("Person-1"), "partner", &select("pending"))
        .unwrap()
        .remove(0);
    tx.disconnect(person, &select("Person-1"), "optional", &select("pending"))
        .unwrap()
        .remove(0);
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(
        person,
        &select("Person-1"),
        &object(&[("partnerId", "pending".into())]),
    )
    .unwrap();
    let rows = tx
        .connect(person, &select("Person-1"), "partner", &select("pending"))
        .unwrap();
    assert_eq!(rows.len(), 1);
    let slot = store.snapshot().ir().models()[person.0].slots["partnerId"];
    assert_eq!(rows[0].get(slot), Some(&Value::from("pending")));
    assert!(tx.prepare().is_err());
}

#[test]
fn mr_12_relation_edit_and_update_produce_equal_records_indices_and_registration_order() {
    let (store, person, _) = network();
    let mut relation = store.begin(CallScope::MutationRoot).unwrap();
    let mut update = store.begin(CallScope::MutationRoot).unwrap();
    relation
        .connect(person, &select("Person-1"), "optional", &select("Group-3"))
        .unwrap()
        .remove(0);
    update
        .update(
            person,
            &select("Person-1"),
            &object(&[("optionalIds", Value::List(vec!["Group-3".into()]))]),
        )
        .unwrap();
    let (relation, update) = (relation.prepare().unwrap(), update.prepare().unwrap());
    let (a, b) = (relation.snapshot(), update.snapshot());
    assert_eq!(
        a.record(person, "Person-1")
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        b.record(person, "Person-1")
            .unwrap()
            .values()
            .collect::<Vec<_>>()
    );
    assert_eq!(a.state.relations, b.state.relations);
    for (a, b) in a.state.models.iter().zip(&b.state.models) {
        assert_eq!(a.next_order, b.next_order);
    }
}
