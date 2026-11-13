//! Apply update-equivalent immediate and final relation constraints, permissions, budgets, and cancellation.

use crate::store::test_selection::id as select;

use super::tests::{ir, model, object};
use super::tests_relation_edit::network;
use super::{CallScope, Store};
use crate::cancellation::Cancellation;
use chigiri::core::Value;
use std::time::{Duration, Instant};

#[test]
fn mr_11_13_14_invalid_relation_options_and_read_scope_prevent_commit() {
    let (store, person, group) = network();
    let oversized = "x".repeat(4097);
    for (model, owner, relation, target, connect) in [
        (person, "Person-1", "groupIds", "Group-1", true),
        (person, "Person-1", "unknown", "Group-1", false),
        (person, "missing", "unknown", "missing", true),
        (person, "Person-1", "groups", oversized.as_str(), false),
        (person, oversized.as_str(), "groups", "Group-1", true),
    ] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.create(group, &object(&[("id", "Group-4".into())]))
            .unwrap();
        let result = if connect {
            tx.connect(model, &select(owner), relation, &select(target))
        } else {
            tx.disconnect(model, &select(owner), relation, &select(target))
        };
        assert!(result.is_err(), "{owner}/{relation}");
        assert!(tx.prepare().is_err());
        assert_eq!(store.snapshot().size().0, 5);
    }
    for invalid in [
        object(&[("limit", Value::Null)]),
        object(&[("where", super::tests::leaf("id", "eq", 1.into()))]),
    ] {
        for (source, target) in [
            (&invalid, &select("missing")),
            (&select("missing"), &invalid),
        ] {
            let mut tx = store.begin(CallScope::MutationRoot).unwrap();
            assert!(tx.connect(person, source, "groups", target).is_err());
            assert!(tx.prepare().is_err());
        }
    }
    for scope in [CallScope::Query, CallScope::Nested] {
        for connect in [false, true] {
            let mut tx = store.begin(scope).unwrap();
            let result = if connect {
                tx.connect(person, &select("Person-1"), "groups", &select("Group-1"))
            } else {
                tx.disconnect(person, &select("Person-1"), "groups", &select("Group-3"))
            };
            assert!(result.is_err());
            assert!(tx.prepare().is_err());
        }
    }
}

#[test]
fn mr_12_owner_list_bounds_remain_immediate_and_inverse_bounds_remain_deferred() {
    let ir = ir(r#"
      type Owner { id:ID! links:[Target!]! @relation(fields:["ids"],references:["id"]) @constraint(minItems:1,maxItems:2) }
      type Target { id:ID! owners:[Owner!]! @relation @constraint(minItems:1) }
      type Query { x:Int }
    "#);
    let (owner, target) = (model(&ir, "Owner"), model(&ir, "Target"));
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(target, &object(&[("id", "Target-1".into())]))
        .unwrap();
    tx.create(target, &object(&[("id", "Target-2".into())]))
        .unwrap();
    tx.create(
        owner,
        &object(&[
            ("id", "Owner-1".into()),
            (
                "ids",
                Value::List(vec!["Target-1".into(), "Target-2".into()]),
            ),
        ]),
    )
    .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    for ids in [
        vec![],
        vec!["Target-1".into(), "Target-2".into(), "Target-3".into()],
    ] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert!(
            tx.update(
                owner,
                &select("missing"),
                &object(&[("ids", Value::List(ids))])
            )
            .is_err()
        );
        assert!(tx.prepare().is_err());
    }
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(target, &object(&[("id", "Target-3".into())]))
        .unwrap();
    assert!(
        tx.connect(owner, &select("Owner-1"), "links", &select("Target-3"))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.disconnect(owner, &select("Owner-1"), "links", &select("Target-1"))
        .unwrap()
        .remove(0);
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.disconnect(owner, &select("Owner-1"), "links", &select("Target-1"))
        .unwrap()
        .remove(0);
    assert!(
        tx.disconnect(owner, &select("Owner-1"), "links", &select("Target-2"))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.disconnect(owner, &select("Owner-1"), "links", &select("Target-1"))
        .unwrap()
        .remove(0);
    tx.delete(target, &select("Target-1")).unwrap();
    tx.prepare().unwrap();
}

#[test]
fn mr_12_13_inverse_uniqueness_is_checked_at_prepare_and_failure_restores_all_models() {
    let (store, person, group) = network();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(group, &object(&[("id", "Group-4".into())]))
        .unwrap();
    tx.connect(person, &select("Person-1"), "partner", &select("Person-1"))
        .unwrap()
        .remove(0);
    tx.connect(person, &select("Person-2"), "partner", &select("Person-1"))
        .unwrap()
        .remove(0);
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 5);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(
        tx.create(group, &object(&[("id", "Group-4".into())]))
            .unwrap()
            .row(),
        3
    );
    tx.prepare().unwrap();
}

#[test]
fn mr_13_15_noop_shares_work_and_cancel_after_edit_refuses_preparation() {
    let (store, person, _) = network();
    for connect in [false, true] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.work.charge(crate::api::WORK_LIMIT - 20).unwrap();
        let result = if connect {
            tx.connect(person, &select("Person-1"), "groups", &select("Group-1"))
        } else {
            tx.disconnect(person, &select("Person-1"), "groups", &select("Group-3"))
        };
        assert!(result.is_err());
        assert!(tx.prepare().is_err());
    }
    let cancel = Cancellation::default();
    let shutdown = Cancellation::default();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.execution_guard(&cancel, &shutdown, Instant::now() + Duration::from_secs(1));
    tx.connect(person, &select("Person-1"), "groups", &select("Group-3"))
        .unwrap()
        .remove(0);
    cancel.cancel();
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 5);
}

#[test]
fn mr_15_connect_enforces_1000_items_while_existing_edge_remains_a_noop() {
    let ir = ir(
        "type Node{id:ID! links:[Node]! @relation(fields:[\"ids\"],references:[\"id\"])} type Query{x:Int}",
    );
    let node = model(&ir, "Node");
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    for id in ["Node-1", "Node-2"] {
        tx.create(
            node,
            &object(&[("id", id.into()), ("ids", Value::List(vec![]))]),
        )
        .unwrap();
    }
    tx.update(
        node,
        &select("Node-1"),
        &object(&[(
            "ids",
            Value::List(
                std::iter::once("Node-1".into())
                    .chain(std::iter::repeat_n(Value::Null, 999))
                    .collect(),
            ),
        )]),
    )
    .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.connect(node, &select("Node-1"), "links", &select("Node-1"))
        .unwrap()
        .remove(0);
    assert!(
        tx.connect(node, &select("Node-1"), "links", &select("Node-2"))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.disconnect(node, &select("Node-1"), "links", &select("Node-1"))
        .unwrap()
        .remove(0);
    tx.connect(node, &select("Node-1"), "links", &select("Node-2"))
        .unwrap()
        .remove(0);
    tx.prepare().unwrap();
}
