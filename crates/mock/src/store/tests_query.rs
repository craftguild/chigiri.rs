//! Verifies query-language semantics with exact ID order and counts so find and
//! count cannot drift apart.

use crate::store::test_selection::id as select;

use super::tests::{ir, leaf, model, object, row};
use super::{CallScope, Store};
use crate::ir::ModelId;
use chigiri::core::{Object, Value};

pub(super) const SDL: &str = r#"
scalar Title @representation(type:STRING,minBytes:10)
scalar Natural @representation(type:INTEGER,min:0)
scalar Json @representation(type:JSON)
enum Phase { Z A }
type Item {
  id:String! title:Title! score:Natural! rank:Int flag:Boolean! phase:Phase!
  all:String any:String op:String tags:[String!] json:Json
}
type Query { items:[Item!]! }
"#;

pub(super) fn seeded() -> (Store, ModelId) {
    let ir = ir(SDL);
    let model = model(&ir, "Item");
    let rows = [
        ("a", "Rust alpha", 0, Value::Null, true, "Z"),
        ("b", "Rust beta.", 5, 2.into(), false, "A"),
        ("c", "abcdefghij", 9, 1.into(), false, "Z"),
        ("d", "Rust alpha", 5, Value::Null, false, "A"),
    ]
    .into_iter()
    .map(|(id, title, score, rank, flag, phase)| {
        row(&[
            ("id", id.into()),
            ("title", title.into()),
            ("score", score.into()),
            ("rank", rank),
            ("flag", flag.into()),
            ("phase", phase.into()),
            ("all", "x".into()),
            ("any", "y".into()),
            ("op", "z".into()),
        ])
    })
    .collect();
    (
        Store::seeded(ir, &object(&[("Item", Value::List(rows))])).unwrap(),
        model,
    )
}

pub(super) fn ids(store: &Store, model: ModelId, options: &Object) -> Vec<String> {
    store
        .snapshot()
        .find(model, options)
        .unwrap()
        .iter()
        .map(|record| record.id().unwrap().to_owned())
        .collect()
}

#[test]
fn mr_03_all_operators_do_not_apply_storage_bounds_to_operands() {
    let (store, model) = seeded();
    for (field, op, value, expected) in [
        ("title", "contains", "Ru".into(), vec!["a", "b", "d"]),
        ("title", "contains", "beta".into(), vec!["b"]),
        ("title", "contains", "rust".into(), vec![]),
        ("title", "contains", "".into(), vec!["a", "b", "c", "d"]),
        ("score", "eq", 5.into(), vec!["b", "d"]),
        ("score", "ne", 5.into(), vec!["a", "c"]),
        ("score", "lt", 5.into(), vec!["a"]),
        ("score", "lte", 5.into(), vec!["a", "b", "d"]),
        ("score", "gt", 5.into(), vec!["c"]),
        ("score", "gte", 5.into(), vec!["b", "c", "d"]),
        ("score", "gt", (-1).into(), vec!["a", "b", "c", "d"]),
        (
            "score",
            "in",
            Value::List(vec![0.into(), 9.into(), 9.into()]),
            vec!["a", "c"],
        ),
        ("score", "in", Value::List(vec![]), vec![]),
        ("id", "contains", "".into(), vec!["a", "b", "c", "d"]),
    ] {
        let options = object(&[("where", leaf(field, op, value))]);
        assert_eq!(ids(&store, model, &options), expected, "{field}/{op}");
        assert_eq!(
            store.snapshot().count(model, &options).unwrap(),
            expected.len()
        );
    }
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update(model, &select("a"), &object(&[("title", "Ru".into())]))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update(model, &select("a"), &object(&[("score", (-1).into())]))
            .is_err()
    );
    assert!(tx.prepare().is_err());
}

#[test]
fn mr_04_05_null_and_reserved_field_names_have_one_predicate_meaning() {
    let (mut store, model) = seeded();
    for (op, value, expected) in [
        ("ne", 1.into(), vec!["b"]),
        ("isNull", true.into(), vec!["a", "d"]),
        ("isNull", false.into(), vec!["b", "c"]),
    ] {
        let options = object(&[("where", leaf("rank", op, value))]);
        assert_eq!(ids(&store, model, &options), expected);
    }
    for (field, value) in [("all", "x"), ("any", "y"), ("op", "z")] {
        assert_eq!(
            ids(
                &store,
                model,
                &object(&[("where", leaf(field, "eq", value.into()))])
            )
            .len(),
            4
        );
    }
    for (key, expected) in [("all", 4), ("any", 0)] {
        let options = object(&[("where", row(&[(key, Value::List(vec![]))]))]);
        assert_eq!(store.snapshot().count(model, &options).unwrap(), expected);
    }
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let record = tx
        .create(
            model,
            &object(&[
                ("id", "e".into()),
                ("title", "abcdefghij".into()),
                ("score", 0.into()),
                ("flag", false.into()),
                ("phase", "Z".into()),
            ]),
        )
        .unwrap();
    let id = record.id().unwrap().to_owned();
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(
        ids(
            &store,
            model,
            &object(&[("where", leaf("rank", "isNull", true.into()))])
        ),
        ["a", "d", &id]
    );
}

pub(super) fn order(field: &str, direction: &str) -> Value {
    row(&[("field", field.into()), ("direction", direction.into())])
}

#[test]
fn mr_06_filter_sort_page_and_stable_null_ties_follow_registration_order() {
    let (mut store, model) = seeded();
    for (keys, expected) in [
        (vec![order("phase", "asc")], vec!["b", "d", "a", "c"]),
        (vec![order("phase", "desc")], vec!["a", "c", "b", "d"]),
        (
            vec![order("rank", "desc"), order("flag", "asc")],
            vec!["b", "c", "d", "a"],
        ),
        (
            vec![order("rank", "asc"), order("flag", "asc")],
            vec!["c", "b", "d", "a"],
        ),
        (vec![order("title", "asc")], vec!["a", "d", "b", "c"]),
        (vec![], vec!["a", "b", "c", "d"]),
    ] {
        assert_eq!(
            ids(&store, model, &object(&[("orderBy", Value::List(keys))])),
            expected
        );
    }
    let options = object(&[
        ("where", leaf("score", "gte", 5.into())),
        ("orderBy", Value::List(vec![order("score", "desc")])),
        ("offset", 1.into()),
        ("limit", 1.into()),
    ]);
    assert_eq!(ids(&store, model, &options), ["b"]);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(model, &select("b"), &object(&[("score", 9.into())]))
        .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(
        ids(
            &store,
            model,
            &object(&[("orderBy", Value::List(vec![order("score", "desc")]))])
        ),
        ["b", "c", "d", "a"]
    );
    assert!(ids(&store, model, &object(&[("offset", 1000.into())])).is_empty());
}

#[test]
fn mr_03_04_07_invalid_conditions_are_checked_before_scan_limit_and_short_circuit() {
    let (store, model) = seeded();
    let invalid = [
        row(&[]),
        row(&[("title", "x".into())]),
        leaf("missing", "eq", 0.into()),
        leaf("title.path", "eq", "x".into()),
        leaf("score", "eq", Value::Null),
        leaf("score", "eq", "5".into()),
        leaf("score", "eq", Value::Float(0.5)),
        leaf("score", "eq", Value::Float(f64::NAN)),
        leaf("flag", "lt", true.into()),
        leaf("phase", "contains", "A".into()),
        leaf("phase", "eq", "a".into()),
        leaf("phase", "eq", "UNKNOWN".into()),
        leaf("tags", "isNull", true.into()),
        leaf("json", "isNull", true.into()),
        leaf("rank", "isNull", 1.into()),
        leaf("rank", "in", Value::List(vec![Value::Null])),
        row(&[("all", Value::List(vec![])), ("any", Value::List(vec![]))]),
        row(&[("field", "rank".into()), ("op", "eq".into())]),
    ];
    let empty = Store::empty(store.snapshot().ir().clone());
    for predicate in invalid {
        for source in [&store, &empty] {
            let options = object(&[("where", predicate.clone()), ("limit", 0.into())]);
            assert!(
                source.snapshot().find(model, &options).is_err(),
                "{predicate:?}"
            );
            let branch = row(&[(
                "any",
                Value::List(vec![
                    row(&[("all", Value::List(vec![]))]),
                    predicate.clone(),
                ]),
            )]);
            assert!(
                source
                    .snapshot()
                    .count(model, &object(&[("where", branch)]))
                    .is_err()
            );
        }
    }
    for (key, value) in [
        ("offset", (-1).into()),
        ("offset", Value::Int(1i128 << 53)),
        ("offset", Value::Float(1.5)),
        ("offset", Value::Null),
        ("limit", Value::Null),
        ("orderBy", Value::Null),
        (
            "orderBy",
            Value::List(vec![order("rank", "asc"), order("rank", "desc")]),
        ),
        ("orderBy", Value::List(vec![order("json", "asc")])),
        ("orderBy", Value::List(vec![order("title", "ASC")])),
    ] {
        assert!(
            store
                .snapshot()
                .find(model, &object(&[(key, value)]))
                .is_err()
        );
    }
    for key in ["orderBy", "offset", "limit", "unknown"] {
        assert!(
            store
                .snapshot()
                .count(model, &object(&[(key, Value::Null)]))
                .is_err()
        );
    }
}
