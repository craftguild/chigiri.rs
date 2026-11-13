//! Test common CRUD ordering, presence, identity, and whole-model failure paths.

use crate::store::test_selection::id as select;

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, Store};
use crate::ir::{FieldId, MockIr, ModelId};

pub(super) const BLOG: &str = include_str!("../../fixtures/blog.graphql");

pub(super) fn ir(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("store.graphql", source)]).unwrap())
}
pub(super) fn model(ir: &MockIr, name: &str) -> ModelId {
    ir.model_for(ir.graph().type_id(name).unwrap()).unwrap()
}
pub(super) fn field(ir: &MockIr, parent: &str, name: &str) -> FieldId {
    ir.graph().types()[ir.graph().type_id(parent).unwrap().0].fields[name]
}
pub(super) fn object(values: &[(&str, Value)]) -> Object {
    values.iter().cloned().collect()
}
pub(super) fn row(values: &[(&str, Value)]) -> Value {
    Value::Object(object(values))
}
pub(super) fn leaf(field: &str, op: &str, value: Value) -> Value {
    row(&[("field", field.into()), ("op", op.into()), ("value", value)])
}
pub(super) fn all(values: &[(&str, Value)]) -> Value {
    row(&[(
        "all",
        Value::List(
            values
                .iter()
                .map(|(field, value)| leaf(field, "eq", value.clone()))
                .collect(),
        ),
    )])
}
pub(super) fn name(value: &str) -> Object {
    object(&[("name", value.into())])
}

fn named(id: &str, name: &str) -> Object {
    object(&[("id", id.into()), ("name", name.into())])
}

#[test]
fn rsm_09_find_is_ordered_and_filters_are_and_with_exact_closed_names() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let mut store = Store::empty(ir.clone());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    for index in 0..25 {
        tx.create(
            user,
            &named(
                &format!("User-{}", index + 1),
                if index % 2 == 0 { "A" } else { "B" },
            ),
        )
        .unwrap();
    }
    store.commit(tx.prepare().unwrap()).unwrap();
    let snapshot = store.snapshot();
    let rows = snapshot.find(user, &Object::new()).unwrap();
    assert_eq!(rows.len(), 25);
    assert_eq!(rows[0].id().unwrap(), "User-1");
    assert_eq!(rows[19].id().unwrap(), "User-20");
    let options = object(&[
        (
            "where",
            all(&[("id", "User-3".into()), ("name", "A".into())]),
        ),
        ("limit", 100.into()),
    ]);
    assert_eq!(
        snapshot.find(user, &options).unwrap()[0].id().unwrap(),
        "User-3"
    );
    let options = object(&[(
        "where",
        all(&[("id", "User-3".into()), ("name", "B".into())]),
    )]);
    assert!(snapshot.find(user, &options).unwrap().is_empty());
    let options = object(&[("where", Value::Null), ("limit", 100.into())]);
    assert_eq!(snapshot.find(user, &options).unwrap().len(), 25);
    assert!(
        snapshot
            .find(user, &object(&[("limit", 0.into())]))
            .unwrap()
            .is_empty()
    );
    for options in [
        object(&[("limit", (-1).into())]),
        object(&[("limit", Value::Int(9_007_199_254_740_992))]),
        object(&[("limit", Value::Float(1.5))]),
        object(&[("limits", 1.into())]),
        object(&[("where", leaf("missing", "isNull", true.into()))]),
        object(&[("where", leaf("posts", "isNull", true.into()))]),
        object(&[("where", row(&[("name", "A".into())]))]),
    ] {
        assert!(snapshot.find(user, &options).is_err());
    }
}

#[test]
fn rsm_09_explicit_id_reuse_after_delete_keeps_new_registration_order_and_old_snapshot() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let seed = object(&[(
        "User",
        Value::List(vec![row(&[("id", "User-1".into()), ("name", "A".into())])]),
    )]);
    let mut store = Store::seeded(ir, &seed).unwrap();
    let old = store.snapshot();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.delete(user, &select("User-1")).unwrap(), 1);
    assert_eq!(tx.delete(user, &select("missing")).unwrap(), 0);
    let created = tx.create(user, &named("User-1", "B")).unwrap();
    assert_eq!(created.id().unwrap(), "User-1");
    assert_eq!(created.row(), 1);
    tx.create(user, &named("User-3", "C")).unwrap();
    tx.update(user, &select("User-1"), &name("changed"))
        .unwrap();
    let snapshot = store.commit(tx.prepare().unwrap()).unwrap();
    let rows = snapshot.find(user, &Object::new()).unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id().unwrap()).collect::<Vec<_>>(),
        ["User-1", "User-3"]
    );
    assert_eq!(
        old.find(user, &Object::new()).unwrap()[0].id().unwrap(),
        "User-1"
    );
}

#[test]
fn rsm_12_failure_discards_all_models_identity_and_order_and_read_only_writes_fail() {
    let ir = ir(BLOG);
    let (user, post) = (model(&ir, "User"), model(&ir, "Post"));
    let mut store = Store::empty(ir);
    for scope in [CallScope::Query, CallScope::Nested] {
        let mut tx = store.begin(scope).unwrap();
        assert!(tx.create(user, &named("User-1", "forbidden")).is_err());
        assert!(tx.prepare().is_err());
    }
    for invalid in [
        object(&[("id", "changed".into())]),
        object(&[("name", Value::Null)]),
        object(&[("posts", Value::List(vec![]))]),
    ] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert_eq!(tx.create(user, &named("User-1", "A")).unwrap().row(), 0);
        tx.create(
            post,
            &object(&[
                ("id", "Post-1".into()),
                ("title", "T".into()),
                ("body", "B".into()),
                ("authorId", "User-1".into()),
            ]),
        )
        .unwrap();
        assert!(tx.update(user, &select("User-1"), &invalid).is_err());
        assert!(tx.prepare().is_err());
        assert_eq!(store.snapshot().size().0, 0);
    }
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.create(user, &named("User-1", "A")).unwrap().row(), 0);
    store.commit(tx.prepare().unwrap()).unwrap();
    let mut failed = store.begin(CallScope::MutationRoot).unwrap();
    failed.create(user, &named("User-2", "B")).unwrap();
    failed.fail();
    assert!(failed.prepare().is_err());
    let mut next = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(next.create(user, &named("User-2", "C")).unwrap().row(), 1);
}

#[test]
fn rsm_13_snapshots_are_immutable_and_stale_or_foreign_candidates_do_not_commit() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let mut store = Store::empty(ir.clone());
    let mut first = store.begin(CallScope::MutationRoot).unwrap();
    let mut stale = store.begin(CallScope::MutationRoot).unwrap();
    first.create(user, &named("User-1", "first")).unwrap();
    stale.create(user, &named("User-1", "stale")).unwrap();
    let snapshot = store.commit(first.prepare().unwrap()).unwrap();
    assert!(store.commit(stale.prepare().unwrap()).is_err());
    let foreign = Store::empty(ir)
        .begin(CallScope::MutationRoot)
        .unwrap()
        .prepare()
        .unwrap();
    assert!(store.commit(foreign).is_err());
    let mut second = store.begin(CallScope::MutationRoot).unwrap();
    second.create(user, &named("User-2", "second")).unwrap();
    store.commit(second.prepare().unwrap()).unwrap();
    assert_eq!(snapshot.size().0, 1);
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn rsm_13_candidates_share_unchanged_records_and_copy_only_changed_records() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let mut store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.create(user, &named("User-1", "A")).unwrap();
    tx.create(user, &named("User-2", "B")).unwrap();
    let first = store.commit(tx.prepare().unwrap()).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(Arc::ptr_eq(
        first.record(user, "User-1").unwrap(),
        tx.record(user, "User-1").unwrap()
    ));
    tx.update(user, &select("User-1"), &name("changed"))
        .unwrap();
    let second = store.commit(tx.prepare().unwrap()).unwrap();
    assert!(!Arc::ptr_eq(
        first.record(user, "User-1").unwrap(),
        second.record(user, "User-1").unwrap()
    ));
    assert!(Arc::ptr_eq(
        first.record(user, "User-2").unwrap(),
        second.record(user, "User-2").unwrap()
    ));
}
