//! Test simple and composite keys, named seed references, implicit relations, and bidirectional edits.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, Related, Store};
use crate::ir::{FieldId, MockIr, ModelId};

fn schema(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("keys.graphql", source)]).unwrap())
}
fn json(source: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(source).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}
fn field(ir: &MockIr, model: &str, name: &str) -> FieldId {
    ir.graph().types()[ir.graph().type_id(model).unwrap().0].fields[name]
}
fn model(ir: &MockIr, name: &str) -> ModelId {
    ir.model_for(ir.graph().type_id(name).unwrap()).unwrap()
}
fn related(store: &Store, parent: &str, name: &str, row: u64) -> Related {
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), parent, name);
    super::traversal::read_row(
        snapshot.ir(),
        &snapshot.state,
        field,
        row,
        &mut super::Work::default(),
    )
    .unwrap()
}
fn rows(value: Related) -> Vec<u64> {
    match value {
        Related::Null => Vec::new(),
        Related::One(record) => vec![record.row()],
        Related::Many(records) => records
            .into_iter()
            .flatten()
            .map(|record| record.row())
            .collect(),
    }
}

const COMPOSITE: &str = r#"
    type User { tenant: ID! code: Int! posts: [Post!]! @relation }
    type Post {
        title: String!
        tenantFK: ID
        author: User @relation(fields:["tenantFK", "codeFK"], references:["tenant", "code"])
    }
    type Query { posts: [Post!]! }
"#;

#[test]
fn sd_keys_composite_uniqueness_is_per_tuple_and_seed_references_are_named() {
    let ir = schema(COMPOSITE);
    let seed = json(
        r#"{
        "Post":[
            {"title":"first", "author":{"code":1,"tenant":"same"}},
            {"title":"second", "tenantFK":"same", "codeFK":2}
        ],
        "User":[{"tenant":"same", "code":1}, {"tenant":"same", "code":2}]
    }"#,
    );
    let store = Store::seeded(ir.clone(), &seed).unwrap();
    assert_eq!(rows(related(&store, "Post", "author", 0)), [0]);
    assert_eq!(rows(related(&store, "Post", "author", 1)), [1]);
    assert_eq!(rows(related(&store, "User", "posts", 0)), [0]);
    assert_eq!(rows(related(&store, "User", "posts", 1)), [1]);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.create(model(&ir, "User"), &json(r#"{"tenant":"same", "code":1}"#))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update_row(model(&ir, "User"), 0, &json(r#"{"code":1}"#))
            .is_err()
    );
    assert_eq!(store.snapshot().size().0, 4);
    for reference in [
        r#"{"tenant":"same"}"#,
        r#"{"tenant":"same", "code":1, "extra":0}"#,
        r#"["same",1]"#,
        r#"{"tenant":"same", "code":"1"}"#,
        r#"{"tenant":"same", "code":null}"#,
    ] {
        let bad = json(&format!(
            r#"{{"User":[{{"tenant":"same","code":1}}],"Post":[{{"title":"bad","author":{reference}}}]}}"#
        ));
        assert!(Store::seeded(ir.clone(), &bad).is_err(), "{reference}");
    }
}

#[test]
fn sd_keys_nullable_composite_requires_all_or_none_and_hidden_public_share_one_value() {
    let ir = schema(COMPOSITE);
    for row in [
        r#"{"title":"empty"}"#,
        r#"{"title":"empty", "author":null}"#,
        r#"{"title":"empty", "author":{"tenant":null,"code":null}}"#,
        r#"{"title":"empty", "tenantFK":null,"codeFK":null}"#,
    ] {
        let seed = json(&format!(r#"{{"User":[],"Post":[{row}]}}"#));
        let store = Store::seeded(ir.clone(), &seed).unwrap();
        assert!(matches!(
            related(&store, "Post", "author", 0),
            Related::Null
        ));
    }
    for row in [
        r#"{"title":"bad", "tenantFK":"same"}"#,
        r#"{"title":"bad", "codeFK":1}"#,
        r#"{"title":"bad", "author":null, "tenantFK":null}"#,
    ] {
        let seed = json(&format!(r#"{{"User":[],"Post":[{row}]}}"#));
        assert!(Store::seeded(ir.clone(), &seed).is_err());
    }
    let mut store = Store::seeded(
        ir.clone(),
        &json(r#"{"User":[{"tenant":"a","code":1}],"Post":[{"title":"empty"}]}"#),
    )
    .unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.edit_edge(field(&ir, "User", "posts"), 0, 0, true)
        .unwrap();
    let prepared = tx.prepare().unwrap();
    store.commit(prepared).unwrap();
    let snapshot = store.snapshot();
    let post = &snapshot.state.models[model(&ir, "Post").0].records[&0];
    let slots = &ir.models()[model(&ir, "Post").0].slots;
    assert_eq!(post.get(slots["tenantFK"]).unwrap().as_str(), Some("a"));
    assert_eq!(post.get(slots["codeFK"]), Some(&Value::Int(1)));
    assert_eq!(rows(related(&store, "Post", "author", 0)), [0]);
}

const IMPLICIT: &str = r#"
    type A { id: Int! bs: [B!]! @relation }
    type B { id: Boolean! as: [A!]! @relation }
    type Query { a: [A!]! }
"#;

#[test]
fn sd_keys_implicit_seed_unilateral_complete_and_conflicting_memberships() {
    let ir = schema(IMPLICIT);
    for seed in [
        r#"{"A":[{"id":1},{"id":2}],"B":[{"id":true,"as":[1,2]}]}"#,
        r#"{"A":[{"id":1,"bs":[true]},{"id":2,"bs":[true]}],"B":[{"id":true}]}"#,
        r#"{"A":[{"id":1,"bs":[true]},{"id":2}],"B":[{"id":true,"as":[1,2]}]}"#,
    ] {
        let store = Store::seeded(ir.clone(), &json(seed)).unwrap();
        assert_eq!(rows(related(&store, "A", "bs", 0)), [0]);
        assert_eq!(rows(related(&store, "B", "as", 0)), [0, 1]);
        assert_eq!(store.snapshot().size().0, 3);
    }
    for seed in [
        r#"{"A":[{"id":1,"bs":[]}],"B":[{"id":true,"as":[1]}]}"#,
        r#"{"A":[{"id":1,"bs":[true]}],"B":[{"id":true,"as":[]}]}"#,
        r#"{"A":[{"id":1,"bs":[true,true]}],"B":[{"id":true}]}"#,
        r#"{"A":[{"id":1,"bs":[false]}],"B":[{"id":true}]}"#,
        r#"{"A":[{"id":1,"bs":["true"]}],"B":[{"id":true}]}"#,
        r#"{"A":[{"id":1,"bs":null}],"B":[{"id":true}]}"#,
    ] {
        assert!(Store::seeded(ir.clone(), &json(seed)).is_err(), "{seed}");
    }
    let empty = Store::seeded(
        ir.clone(),
        &json(r#"{"A":[{"id":1,"bs":[]}],"B":[{"id":true}]}"#),
    )
    .unwrap();
    assert!(rows(related(&empty, "B", "as", 0)).is_empty());
}

#[test]
fn sd_keys_implicit_both_sides_edit_and_delete_only_cleans_links() {
    let ir = schema(IMPLICIT);
    let mut store =
        Store::seeded(ir.clone(), &json(r#"{"A":[{"id":1}],"B":[{"id":true}]}"#)).unwrap();
    let a = field(&ir, "A", "bs");
    let b = field(&ir, "B", "as");
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.edit_edge(b, 0, 0, true).unwrap();
    tx.edit_edge(a, 0, 0, true).unwrap();
    let prepared = tx.prepare().unwrap();
    store.commit(prepared).unwrap();
    assert_eq!(rows(related(&store, "B", "as", 0)), [0]);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.edit_edge(a, 0, 0, false).unwrap();
    let prepared = tx.prepare().unwrap();
    store.commit(prepared).unwrap();
    assert!(rows(related(&store, "B", "as", 0)).is_empty());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.edit_edge(a, 0, 0, true).unwrap();
    tx.delete(
        model(&ir, "A"),
        &json(r#"{"where":{"field":"id","op":"eq","value":1}}"#),
    )
    .unwrap();
    let prepared = tx.prepare().unwrap();
    store.commit(prepared).unwrap();
    assert_eq!(store.snapshot().size().0, 1);
    assert!(rows(related(&store, "B", "as", 0)).is_empty());
    let mut tx = store.begin(CallScope::Query).unwrap();
    assert!(tx.edit_edge(a, 0, 0, true).is_err());
}

#[test]
fn sd_keys_format_scratch_does_not_inflate_retained_record_usage() {
    let plain = schema("type Data { text: String! } type Query { data: Data }");
    let constrained = schema(
        "type Data { text: String! @constraint(format:\"byte\") } type Query { data: Data }",
    );
    let seed = json(r#"{"Data":[{"text":"aGVsbG8="}]}"#);
    let plain = Store::seeded(plain, &seed).unwrap();
    let constrained = Store::seeded(constrained, &seed).unwrap();
    assert_eq!(plain.snapshot().size(), constrained.snapshot().size());
}
