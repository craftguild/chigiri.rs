//! Test implicit conjunction, closed pagination options, and integer bounds in one selection plan.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{Store, StoreError};
use crate::ir::{MockIr, ModelId};

fn fixture() -> (Store, ModelId) {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "selection.graphql",
            "type Entry { rank: Int! group: Int! } type Query { entries: [Entry!]! }",
        )])
        .unwrap(),
    );
    let model = ir.model_for(ir.graph().type_id("Entry").unwrap()).unwrap();
    let rows = (0..150)
        .map(|rank| {
            Value::Object(Object::from_iter([
                ("rank", Value::Int(rank)),
                ("group", Value::Int(rank % 3)),
            ]))
        })
        .collect();
    let seed = Object::from_iter([("Entry", Value::List(rows))]);
    (Store::seeded(ir, &seed).unwrap(), model)
}
fn options(value: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(value).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}
fn ranks(store: &Store, model: ModelId, options: &Object) -> Result<Vec<i128>, StoreError> {
    let snapshot = store.snapshot();
    let slot = snapshot.ir().models()[model.0].slots["rank"];
    snapshot.find(model, options).map(|rows| {
        rows.iter()
            .map(|row| {
                let Some(Value::Int(rank)) = row.get(slot) else {
                    panic!("rank field required")
                };
                *rank
            })
            .collect()
    })
}

#[test]
fn sd_selection_omitted_limit_returns_all_after_offset_without_100_cutoff() {
    let (store, model) = fixture();
    assert_eq!(
        ranks(&store, model, &Object::new()).unwrap(),
        (0..150).collect::<Vec<_>>()
    );
    assert_eq!(
        ranks(&store, model, &options(r#"{"offset":20}"#)).unwrap(),
        (20..150).collect::<Vec<_>>()
    );
    assert_eq!(
        ranks(&store, model, &options(r#"{"limit":120}"#)).unwrap(),
        (0..120).collect::<Vec<_>>()
    );
    assert!(
        ranks(&store, model, &options(r#"{"limit":0}"#))
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 150);
}

#[test]
fn sd_selection_where_sort_offset_limit_and_ties_share_registration_order() {
    let (store, model) = fixture();
    let selected = options(
        r#"{"where":{"field":"rank","op":"gte","value":140},"orderBy":[{"field":"group","direction":"asc"}],"offset":2,"limit":5}"#,
    );
    assert_eq!(
        ranks(&store, model, &selected).unwrap(),
        [147, 142, 145, 148, 140]
    );
    assert!(store.snapshot().count(model, &selected).is_err());
    assert_eq!(
        store
            .snapshot()
            .count(
                model,
                &options(r#"{"where":{"field":"rank","op":"gte","value":140}}"#)
            )
            .unwrap(),
        10
    );
}

#[test]
fn sd_selection_integer_boundaries_are_validated_before_conversion_and_zero_scan() {
    let (store, model) = fixture();
    let maximum = 9_007_199_254_740_991_i128;
    assert_eq!(
        ranks(
            &store,
            model,
            &Object::from_iter([("limit", Value::Int(maximum))])
        )
        .unwrap()
        .len(),
        150
    );
    assert!(
        ranks(
            &store,
            model,
            &Object::from_iter([("offset", Value::Int(maximum))])
        )
        .unwrap()
        .is_empty()
    );
    for value in [
        Value::Int(-1),
        Value::Int(maximum + 1),
        Value::Float(0.5),
        Value::Null,
        Value::from("2"),
    ] {
        for name in ["offset", "limit"] {
            assert!(ranks(&store, model, &Object::from_iter([(name, value.clone())])).is_err());
        }
    }
    assert_eq!(
        ranks(
            &store,
            model,
            &Object::from_iter([("limit", Value::Float(1.0))])
        )
        .unwrap(),
        [0]
    );
    for bad in [
        r#"{"limit":0,"where":{"field":"missing","op":"eq","value":0}}"#,
        r#"{"where":{"any":[]},"unexpected":1}"#,
        r#"{"offset":9007199254740991,"orderBy":[{"field":"rank","direction":"wrong"}]}"#,
    ] {
        assert!(ranks(&store, model, &options(bad)).is_err());
    }
}
