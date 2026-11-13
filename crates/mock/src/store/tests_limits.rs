//! Test storage capacity, work limits, ULID generation, and state preservation after failure.

use crate::store::test_selection::id as select;

use chigiri::core::{Object, Value};

use super::tests::{ir, leaf, model, object, row};
use super::{CallScope, Store};
use crate::value::{MAX_BYTES, MAX_NODES};

#[test]
fn rsm_16_per_model_and_total_record_limits_are_inclusive() {
    let ir = ir("type A{id:ID!} type B{id:ID!} type C{id:ID!} type Query{x:Int}");
    let rows = Value::List(
        (0..1000)
            .map(|index| row(&[("id", format!("seed-{index}").into())]))
            .collect(),
    );
    let mut seed = object(&[("A", rows.clone()), ("B", rows)]);
    let store = Store::seeded(ir.clone(), &seed).unwrap();
    assert_eq!(store.snapshot().size().0, 2000);
    for name in ["A", "C"] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert!(
            tx.create(model(&ir, name), &object(&[("id", "overflow".into())]))
                .is_err()
        );
        assert!(tx.prepare().is_err());
    }
    seed.insert("C", Value::List(vec![row(&[("id", "c".into())])]));
    assert!(Store::seeded(ir.clone(), &seed).is_err());
    let rows = Value::List(
        (0..1001)
            .map(|index| row(&[("id", format!("seed-{index}").into())]))
            .collect(),
    );
    assert!(Store::seeded(ir, &object(&[("A", rows)])).is_err());
}

#[test]
fn rsm_16_store_node_capacity_is_checked_before_candidate_copy() {
    let ir = ir("type Item{id:ID! values:[Int]!} type Query{x:Int}");
    let item = model(&ir, "Item");
    let mut store = Store::empty(ir.clone());
    // Value nodes include the list plus three fields and two row/identity index nodes.
    // Store capacity is independent of the startup or single-request shared budget.
    for index in 0..100 {
        let (id, count) = if index == 99 {
            ("last".to_owned(), 500)
        } else {
            (format!("item-{index}"), 1000)
        };
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.create(
            item,
            &object(&[
                ("id", id.into()),
                ("values", Value::List(vec![Value::Null; count])),
            ]),
        )
        .unwrap();
        store.commit(tx.prepare().unwrap()).unwrap();
    }
    assert_eq!(store.current.state.usage.nodes, MAX_NODES);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update(
            model(&ir, "Item"),
            &select("last"),
            &object(&[("values", Value::List(vec![Value::Null; 501]))])
        )
        .is_err()
    );
    assert!(tx.prepare().is_err());
    assert_eq!(store.current.state.usage.nodes, MAX_NODES);
}

#[test]
fn rsm_16_store_bytes_include_keys_and_stop_at_exact_total() {
    let ir =
        ir("type Item{id:ID! a:String! b:String! c:String! d:String! e:String!} type Query{x:Int}");
    let mut remaining = MAX_BYTES;
    let item = model(&ir, "Item");
    let mut store = Store::empty(ir.clone());
    let mut count = 0;
    while remaining > 0 {
        let mut record = object(&[("id", format!("{count:04}").into())]);
        // Each record uses 12 value bytes, 16 row-index bytes, and 12 identity-index bytes.
        let mut payload = (remaining - 40).min(5 * 4096);
        let bytes = payload + 40;
        for name in ["a", "b", "c", "d", "e"] {
            let length = payload.min(4096);
            record.insert(name, "x".repeat(length));
            payload -= length;
        }
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.create(item, &record).unwrap();
        store.commit(tx.prepare().unwrap()).unwrap();
        count += 1;
        remaining -= bytes;
    }
    let last = format!("{:04}", count - 1);
    assert_eq!(store.snapshot().size().1, MAX_BYTES);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update(
            model(&ir, "Item"),
            &select(&last),
            &object(&[("e", "x".into())])
        )
        .is_err()
    );
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().1, MAX_BYTES);
}

#[test]
fn rsm_16_find_counts_empty_options_nulls_and_shared_work_and_sticks_after_failure() {
    let ir = ir("type Item{id:ID!} type Query{x:Int}");
    let item = model(&ir, "Item");
    let store = Store::empty(ir);
    let mut tx = store.begin(CallScope::Query).unwrap();
    for _ in 0..MAX_NODES / 2 {
        tx.find(item, &Object::new()).unwrap();
    }
    assert_eq!(tx.values.nodes(), MAX_NODES);
    assert_eq!(tx.values.bytes(), MAX_NODES);
    assert!(tx.find(item, &Object::new()).is_err());
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::Query).unwrap();
    tx.find(
        item,
        &object(&[("where", Value::Null), ("limit", 0.into())]),
    )
    .unwrap();
    assert_eq!(tx.values.nodes(), 4);
    assert_eq!(tx.values.bytes(), 1 + 1 + 1 + 8 + 5 + 5);
    tx.work.charge(super::work::MAX_WORK - 43).unwrap();
    assert!(tx.find(item, &Object::new()).is_err());
    assert!(tx.prepare().is_err());
}

#[test]
fn sd_05_ulid_constraints_and_registration_overflow_are_bounded() {
    for name in ["Model".to_owned(), "Model".repeat(100)] {
        let ir = ir(&format!(
            "type {name}{{id:String! @constraint(minLength:26,maxLength:26)}} type Query{{x:Int}}"
        ));
        let item = model(&ir, &name);
        let store = Store::empty(ir);
        let mut first = store.begin(CallScope::MutationRoot).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..10 {
            let row = first.create(item, &Object::new()).unwrap();
            let id = row.id().unwrap();
            assert_eq!(id.len(), 26);
            assert_eq!(id, id.to_ascii_lowercase());
            assert!(ulid::Ulid::from_string(id).is_ok());
            assert!(seen.insert(id.to_owned()));
        }
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.state.models[item.0].next_order = u64::MAX;
        assert!(tx.create(item, &Object::new()).is_err());
        assert!(tx.prepare().is_err());
        assert_eq!(store.snapshot().size().0, 0);
    }
    for bounds in [
        "maxLength:1",
        "startsWith:\"UPPER\"",
        "pattern:\"^[A-Z]+$\"",
    ] {
        let ir = ir(&format!(
            "type Item{{id:ID! @constraint({bounds})}} type Query{{x:Int}}"
        ));
        let item = model(&ir, "Item");
        let store = Store::empty(ir);
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert!(tx.create(item, &Object::new()).is_err());
        assert!(tx.prepare().is_err());
    }
}

#[test]
fn rsm_16_repeated_real_filter_scans_share_the_rust_work_limit() {
    let fields = (0..10)
        .map(|index| format!("f{index}:Int "))
        .collect::<String>();
    let ir = ir(&format!("type Item{{id:ID! {fields}}} type Query{{x:Int}}"));
    let item = model(&ir, "Item");
    let template = (0..10)
        .map(|index| (format!("f{index}"), Value::Int(0)))
        .collect::<Object>();
    let rows = (0..1000)
        .map(|index| {
            let mut record = template.clone();
            record.insert("id", format!("seed-{index}"));
            Value::Object(record)
        })
        .collect();
    let store = Store::seeded(ir, &object(&[("Item", Value::List(rows))])).unwrap();
    let mut filters = template;
    filters.insert("f9", 1);
    let predicates = filters
        .iter()
        .map(|(name, value)| leaf(name, "eq", value.clone()))
        .collect();
    let options = object(&[
        ("where", row(&[("all", Value::List(predicates))])),
        ("limit", 100.into()),
    ]);
    let mut tx = store.begin(CallScope::Query).unwrap();
    for _ in 0..20 {
        assert!(tx.find(item, &options).unwrap().is_empty());
    }
    let error = (20..256)
        .find_map(|_| tx.find(item, &options).err())
        .expect("the work budget must be exhausted");
    assert_eq!(error.0, "request aggregate limit");
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 1000);
}
