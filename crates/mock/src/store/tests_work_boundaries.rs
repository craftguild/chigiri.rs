//! Use small isolated fixtures to verify exact work charges, one-unit overflow, and sticky failure.

use crate::store::test_selection::id as select;

use super::tests::{ir, leaf, model, object, row};
use super::{CallScope, Store, Transaction};
use crate::api::WORK_LIMIT;
use chigiri::core::{Object, Value};
use std::sync::Arc;

fn remaining(store: &Store, exact: usize, shortage: usize) -> Transaction {
    assert_eq!(WORK_LIMIT, 1_000_000);
    let mut work = super::work::Work::default();
    work.charge(WORK_LIMIT - exact + shortage).unwrap();
    store
        .begin_with_work(CallScope::MutationRoot, work)
        .unwrap()
}

#[test]
fn mr_15_count_predicate_string_contains_and_sort_use_exact_shared_work() {
    let ir = ir("type N {id:ID! s:String!} type Query{x:Int}");
    let n = model(&ir, "N");
    let store = Store::seeded(
        ir,
        &object(&[(
            "N",
            Value::List(vec![
                row(&[("id", "a".into()), ("s", "cd".into())]),
                row(&[("id", "b".into()), ("s", "ab".into())]),
            ]),
        )]),
    )
    .unwrap();
    // Charge the candidate map reservation and node/byte reservations to the same work budget.
    // Eq costs 62 to build and 14 to evaluate; contains costs 66 and 16; all costs 77 and 16.
    // These exact charges follow accounting rules and must not be replaced with fixture-length estimates.
    let cases = [
        ("count", Object::new(), 11, 2),
        (
            "eq",
            object(&[("where", leaf("s", "eq", "ab".into()))]),
            82,
            1,
        ),
        (
            "contains",
            object(&[("where", leaf("s", "contains", "b".into()))]),
            88,
            1,
        ),
        (
            "all",
            object(&[(
                "where",
                row(&[("all", Value::List(vec![leaf("s", "eq", "ab".into())]))]),
            )]),
            99,
            1,
        ),
    ];
    for (name, options, exact, expected) in cases {
        for shortage in [0, 1] {
            let mut tx = remaining(&store, exact, shortage);
            let result = tx.count(n, &options);
            if shortage == 0 {
                assert_eq!(result.unwrap(), expected, "{name}");
                assert!(!tx.failed);
                assert!(tx.work.charge(1).is_err(), "name: work is missing.");
            } else {
                assert!(result.is_err(), "{name}");
                assert!(tx.failed);
                assert!(tx.count(n, &Object::new()).is_err());
                assert!(tx.prepare().is_err());
            }
        }
    }
    let order = object(&[(
        "orderBy",
        Value::List(vec![row(&[
            ("field", "s".into()),
            ("direction", "asc".into()),
        ])]),
    )]);
    // Candidate 6 + options 19 + sort type 45 + root 2 + scan 2 + scratch 2 + merge 8 + items 1 + output 6 = 91.
    for shortage in [0, 1] {
        let mut tx = remaining(&store, 91, shortage);
        let result = tx.find(n, &order);
        if shortage == 0 {
            let rows = result.unwrap();
            assert_eq!(
                rows.iter().map(|row| row.id().unwrap()).collect::<Vec<_>>(),
                ["b", "a"]
            );
            assert!(!tx.failed);
            assert!(tx.work.charge(1).is_err());
        } else {
            assert!(result.is_err());
            assert!(tx.failed);
            assert!(tx.find(n, &Object::new()).is_err());
            assert!(tx.prepare().is_err());
        }
    }
    assert_eq!(store.snapshot().count(n, &Object::new()).unwrap(), 2);
    assert_eq!(
        store.snapshot().find(n, &Object::new()).unwrap()[0]
            .id()
            .unwrap(),
        "a"
    );
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(
        tx.create(n, &object(&[("id", "N-1".into()), ("s", "ok".into())]))
            .unwrap()
            .id()
            .unwrap(),
        "N-1"
    );
    tx.prepare().unwrap();
}

#[test]
fn mr_15_relation_changes_and_noops_use_exact_work_and_rollback_one_short() {
    let ir = ir(r#"
      type N {
        id:ID!
        links:[N]! @relation(fields:["ids"],references:["id"])
        back:[N!]! @relation
      }
      type Query { x:Int }
    "#);
    let n = model(&ir, "N");
    let ids = ir.models()[n.0].slots["ids"];
    let original = Value::List(vec!["b".into(), Value::Null]);
    let store = Store::seeded(
        ir,
        &object(&[(
            "N",
            Value::List(vec![
                row(&[("id", "a".into()), ("ids", original.clone())]),
                row(&[("id", "b".into()), ("ids", Value::List(vec![]))]),
            ]),
        )]),
    )
    .unwrap();
    let old = store.snapshot().find(n, &Object::new()).unwrap()[0].clone();
    // relation name 6 + both sides of the selection 79 each + linear addition 1 + side 1 + target key 3 + FK7 + output 4 = 180
    // Target 0 is a selection of 76 and 165 without marginal processing. Changes include COW in the record-typed key index.
    // Candidate map reservation 8. Connect is additional 7 + record 140 + new old node 20 + index 555, disconnect is 78 + 16 + 539
    // The new ordinal side reserves additional 8 bytes for each side, and the existing side order is kept updated.
    for (connect, target, exact, expected, noop) in [
        (true, "b", 188, original.clone(), true),
        (false, "c", 173, original.clone(), true),
        (
            true,
            "a",
            910,
            Value::List(vec!["b".into(), Value::Null, "a".into()]),
            false,
        ),
        (false, "b", 821, Value::List(vec![Value::Null]), false),
    ] {
        for shortage in [0, 1] {
            let mut tx = remaining(&store, exact, shortage);
            let result = if connect {
                tx.connect(n, &select("a"), "links", &select(target))
            } else {
                tx.disconnect(n, &select("a"), "links", &select(target))
            };
            if shortage == 0 {
                let mut rows = result.unwrap();
                assert_eq!(rows.len(), 1);
                let row = rows.remove(0);
                assert_eq!(row.get(ids), Some(&expected));
                assert_eq!(Arc::ptr_eq(&row, &old), noop);
                assert!(!tx.failed);
                assert!(
                    tx.work.charge(1).is_err(),
                    "an unexpected error kind was returned"
                );
            } else {
                assert!(result.is_err(), "{connect}/{target}");
                assert!(tx.failed);
                assert!(
                    tx.disconnect(n, &select("a"), "links", &select("c"))
                        .is_err()
                );
                assert!(tx.prepare().is_err());
            }
            assert_eq!(
                store.snapshot().find(n, &Object::new()).unwrap()[0].get(ids),
                Some(&original)
            );
            assert_eq!(store.snapshot().count(n, &Object::new()).unwrap(), 2);
        }
    }
    // Successful operation accounting also leaves enough work for the subsequent prepare step.
    // A fresh transaction combines the relation change and creation, then validates the final store.
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.connect(n, &select("a"), "links", &select("a"))
        .unwrap()
        .remove(0);
    assert_eq!(
        tx.create(
            n,
            &object(&[("id", "N-1".into()), ("ids", Value::List(vec![]))])
        )
        .unwrap()
        .id()
        .unwrap(),
        "N-1"
    );
    tx.prepare().unwrap();
}
