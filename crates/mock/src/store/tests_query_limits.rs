//! Test predicate and work-budget boundaries, exact counts, and candidate isolation from snapshots.

use crate::store::test_selection::id as select;

use super::tests::{ir, leaf, model, object, row};
use super::tests_query::{ids, order, seeded};
use super::{CallScope, Store};
use crate::cancellation::Cancellation;
use chigiri::core::{Object, Value};
use std::time::{Duration, Instant};

fn group(key: &str, values: Vec<Value>) -> Value {
    row(&[(key, Value::List(values))])
}
fn nested(depth: usize) -> Value {
    let mut value = leaf("rank", "isNull", true.into());
    for _ in 1..depth {
        value = group("all", vec![value]);
    }
    value
}

#[test]
fn mr_04_07_15_predicate_depth_nodes_candidates_strings_and_sort_have_boundaries() {
    let (store, model) = seeded();
    for (depth, valid) in [(8, true), (9, false)] {
        let options = object(&[("where", nested(depth))]);
        assert_eq!(store.snapshot().count(model, &options).is_ok(), valid);
    }
    for (nodes, valid) in [(128, true), (129, false)] {
        let empty = group("all", vec![]);
        let predicate = group(
            "all",
            vec![
                group("all", vec![empty.clone(); 64]),
                group("all", vec![empty; nodes - 67]),
            ],
        );
        assert_eq!(
            store
                .snapshot()
                .count(model, &object(&[("where", predicate)]))
                .is_ok(),
            valid
        );
    }
    for (length, valid) in [(100, true), (101, false)] {
        let operand = Value::List(vec![5.into(); length]);
        let options = object(&[("where", leaf("score", "in", operand))]);
        assert_eq!(store.snapshot().count(model, &options).is_ok(), valid);
    }
    for (bytes, valid) in [(4096, true), (4097, false)] {
        let options = object(&[("where", leaf("title", "eq", "x".repeat(bytes).into()))]);
        assert_eq!(store.snapshot().count(model, &options).is_ok(), valid);
    }
    let mut keys = vec![
        order("rank", "asc"),
        order("score", "asc"),
        order("phase", "asc"),
        order("id", "asc"),
    ];
    assert!(
        store
            .snapshot()
            .find(model, &object(&[("orderBy", Value::List(keys.clone()))]))
            .is_ok()
    );
    keys.push(order("flag", "asc"));
    assert!(
        store
            .snapshot()
            .find(model, &object(&[("orderBy", Value::List(keys))]))
            .is_err()
    );
}

#[test]
fn mr_05_numeric_comparison_preserves_exact_large_integers_and_rejects_rounding() {
    let ir = ir(
        "scalar Number @representation(type:NUMBER) type Item{id:ID! number:Number!} type Query{x:Int}",
    );
    let model = model(&ir, "Item");
    let n = 1i128 << 53;
    let rows = vec![
        row(&[("id", "b".into()), ("number", Value::Int(n + 2))]),
        row(&[("id", "a".into()), ("number", Value::Int(n))]),
    ];
    let store = Store::seeded(ir, &object(&[("Item", Value::List(rows))])).unwrap();
    assert_eq!(
        ids(
            &store,
            model,
            &object(&[("orderBy", Value::List(vec![order("number", "asc")]))])
        ),
        ["a", "b"]
    );
    assert_eq!(
        ids(
            &store,
            model,
            &object(&[("where", leaf("number", "gt", Value::Int(n)))])
        ),
        ["b"]
    );
    for value in [
        Value::Int(n + 1),
        Value::Float(f64::INFINITY),
        Value::String(n.to_string()),
    ] {
        assert!(
            store
                .snapshot()
                .count(model, &object(&[("where", leaf("number", "eq", value))]))
                .is_err()
        );
    }
}

#[test]
fn mr_08_09_count_and_unlimited_find_read_snapshot_or_latest_candidate() {
    let ir = ir("type Item{id:ID!} type Query{x:Int}");
    let model = model(&ir, "Item");
    let rows = (0..130)
        .map(|i| row(&[("id", format!("item-{i}").into())]))
        .collect();
    let mut store = Store::seeded(ir, &object(&[("Item", Value::List(rows))])).unwrap();
    let mut reader = store.snapshot().reader();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.count(model, &Object::new()).unwrap(), 130);
    let created = tx
        .create(model, &object(&[("id", "created".into())]))
        .unwrap();
    assert_eq!(tx.count(model, &Object::new()).unwrap(), 131);
    assert_eq!(reader.count(model, &Object::new()).unwrap(), 130);
    assert_eq!(tx.find(model, &Object::new()).unwrap().len(), 131);
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 131);
    assert_eq!(reader.count(model, &Object::new()).unwrap(), 130);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.delete(model, &select(created.id().unwrap())).unwrap();
    assert_eq!(tx.count(model, &Object::new()).unwrap(), 130);
    assert!(tx.count(model, &object(&[("limit", 0.into())])).is_err());
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 131);
}

#[test]
fn mr_15_query_work_is_shared_and_cancel_or_deadline_prevents_completion() {
    let (store, model) = seeded();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.work.charge(crate::api::WORK_LIMIT - 13).unwrap();
    assert!(tx.count(model, &Object::new()).is_err());
    assert!(tx.find(model, &Object::new()).is_err());
    assert!(tx.prepare().is_err());
    for expired in [false, true] {
        let cancel = Cancellation::default();
        let shutdown = Cancellation::default();
        let deadline = if expired {
            Instant::now() - Duration::from_millis(1)
        } else {
            Instant::now() + Duration::from_secs(1)
        };
        if !expired {
            cancel.cancel();
        }
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.execution_guard(&cancel, &shutdown, deadline);
        assert!(tx.count(model, &Object::new()).is_err());
        assert!(tx.prepare().is_err());
    }
    assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 4);
}
