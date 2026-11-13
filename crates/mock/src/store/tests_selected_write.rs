//! Test set-based updates, empty selections, RESTRICT deletes, and complete root rollback.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, Store};
use crate::ir::{MockIr, ModelId};

fn schema(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("sets.graphql", source)]).unwrap())
}
fn model(ir: &MockIr, name: &str) -> ModelId {
    ir.model_for(ir.graph().type_id(name).unwrap()).unwrap()
}
fn object(source: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(source).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}
fn empty() -> Object {
    Object::new()
}

#[test]
fn sd_write_public_relation_methods_accept_idless_selection_and_traverse_snapshot_records() {
    let ir = schema(
        r#"type A{tag:Int! peer:B @relation(fields:["bcode"],references:["code"])} type B{code:Int! owners:[A!]! @relation} type Query{x:Int}"#,
    );
    let a = model(&ir, "A");
    let b = model(&ir, "B");
    let peer = ir.graph().types()[ir.models()[a.0].ty.0].fields["peer"];
    let owners = ir.graph().types()[ir.models()[b.0].ty.0].fields["owners"];
    let seed = object(r#"{"A":[{"tag":1},{"tag":2}],"B":[{"code":8}]}"#);
    let mut store = Store::seeded(ir.clone(), &seed).unwrap();
    let before = store.snapshot();
    let original = before.find(a, &empty()).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let connected = tx.connect(a, &empty(), "peer", &empty()).unwrap();
    assert_eq!(connected.len(), 2);
    assert!(connected.iter().all(|record| record.id().is_none()));
    assert!(matches!(
        tx.related(peer, &connected[0]).unwrap(),
        super::Related::One(_)
    ));
    let snapshot = store.commit(tx.prepare().unwrap()).unwrap();
    let mut reader = snapshot.reader();
    assert!(matches!(
        reader.related(peer, &connected[1]).unwrap(),
        super::Related::One(_)
    ));
    assert!(matches!(
        before.related(peer, &original[0]).unwrap(),
        super::Related::Null
    ));
    assert!(before.related(peer, &connected[0]).is_err());
    let foreign = Store::seeded(ir, &seed)
        .unwrap()
        .snapshot()
        .find(a, &empty())
        .unwrap();
    assert!(snapshot.related(peer, &foreign[0]).is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let result = tx.disconnect(b, &empty(), "owners", &empty()).unwrap();
    assert_eq!(result.len(), 1);
    assert!(
        matches!(tx.related(owners, &result[0]).unwrap(), super::Related::Many(rows) if rows.is_empty())
    );
    store.commit(tx.prepare().unwrap()).unwrap();
    assert!(matches!(
        snapshot.related(peer, &connected[0]).unwrap(),
        super::Related::One(_)
    ));
}

#[test]
fn sd_write_update_all_duplicates_and_freezes_selection_before_patch() {
    let ir = schema("type Entry { score: Int! group: Int! } type Query { entries: [Entry!]! }");
    let model = model(&ir, "Entry");
    let rows = (0..150)
        .map(|score| {
            Value::Object(Object::from_iter([
                ("score", Value::Int(score)),
                ("group", Value::Int(score % 3)),
            ]))
        })
        .collect();
    let mut store = Store::seeded(
        ir.clone(),
        &Object::from_iter([("Entry", Value::List(rows))]),
    )
    .unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let selected = object(
        r#"{"where":{"field":"score","op":"gte","value":140},"orderBy":[{"field":"score","direction":"desc"}],"limit":5}"#,
    );
    let changed = tx
        .update(model, &selected, &object(r#"{"score":0}"#))
        .unwrap();
    assert_eq!(
        changed.iter().map(|row| row.row()).collect::<Vec<_>>(),
        [149, 148, 147, 146, 145]
    );
    assert_eq!(
        tx.count(
            model,
            &object(r#"{"where":{"field":"score","op":"eq","value":0}}"#)
        )
        .unwrap(),
        6
    );
    let changed = tx
        .update(model, &empty(), &object(r#"{"group":1}"#))
        .unwrap();
    assert_eq!(changed.len(), 150);
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(
        store
            .snapshot()
            .count(
                model,
                &object(r#"{"where":{"field":"group","op":"eq","value":1}}"#)
            )
            .unwrap(),
        150
    );
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(
        tx.delete(model, &object(r#"{"offset":20,"limit":110}"#))
            .unwrap(),
        110
    );
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().size().0, 40);
}

#[test]
fn sd_write_zero_matches_validate_patch_permissions_and_caught_errors_block_commit() {
    let ir = schema(
        "type Entry { id: Int! score: Int! @constraint(min:0) title: String! @virtual } type Audit { value: Int! } type Query { entries: [Entry!]! }",
    );
    let entry = model(&ir, "Entry");
    let audit = model(&ir, "Audit");
    let store = Store::seeded(ir, &object(r#"{"Entry":[{"id":1,"score":1}],"Audit":[]}"#)).unwrap();
    let none = object(r#"{"where":{"any":[]}}"#);
    for patch in [
        r#"{"unknown":0}"#,
        r#"{"title":"computed"}"#,
        r#"{"id":1}"#,
        r#"{"score":null}"#,
        r#"{"score":-1}"#,
    ] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.update(entry, &empty(), &object(r#"{"score":2}"#))
            .unwrap();
        tx.create(audit, &object(r#"{"value":1}"#)).unwrap();
        assert!(tx.update(entry, &none, &object(patch)).is_err());
        assert!(tx.prepare().is_err());
        assert_eq!(store.snapshot().size().0, 1);
    }
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.update(entry, &none, &object(r#"{"score":0}"#))
            .unwrap()
            .is_empty()
    );
    assert_eq!(tx.delete(entry, &none).unwrap(), 0);
    assert!(tx.prepare().is_ok());
    for scope in [CallScope::Query, CallScope::Nested] {
        let mut tx = store.begin(scope).unwrap();
        assert!(tx.update(entry, &none, &empty()).is_err());
        assert!(tx.prepare().is_err());
    }
}

#[test]
fn sd_write_delete_allows_only_internal_fk_edges_and_implicit_cleanup() {
    let ir = schema(
        r#"
        type Node { id: Int! next: Node! @relation(fields:["nextId"],references:["id"]) }
        type Pin { id: Int! node: Node! @relation(fields:["nodeId"],references:["id"]) }
        type Query { nodes: [Node!]! }
    "#,
    );
    let node = model(&ir, "Node");
    let base = r#"{"Node":[{"id":1,"nextId":2},{"id":2,"nextId":1}],"Pin":[]}"#;
    let mut store = Store::seeded(ir.clone(), &object(base)).unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(tx.delete(node, &object(r#"{"limit":1}"#)).is_err());
    assert!(tx.prepare().is_err());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.delete(node, &empty()).unwrap(), 2);
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().size().0, 0);
    let store = Store::seeded(
        ir,
        &object(&base.replace(r#""Pin":[]"#, r#""Pin":[{"id":1,"nodeId":2}]"#)),
    )
    .unwrap();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(tx.delete(node, &empty()).is_err());
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 3);
}

#[test]
fn sd_write_relation_cartesian_selection_zero_targets_and_single_edge_rollback() {
    let ir = schema(
        r#"
        type A { id: Int! bs: [B!]! @relation }
        type B { id: Int! as: [A!]! @relation }
        type Parent { id: Int! child: Child @relation(fields:["childId"],references:["id"]) }
        type Child { id: Int! }
        type Query { a: [A!]! }
    "#,
    );
    let a = model(&ir, "A");
    let b = model(&ir, "B");
    let parent = model(&ir, "Parent");
    let mut store = Store::seeded(ir, &object(r#"{"A":[{"id":1},{"id":2}],"B":[{"id":1},{"id":2}],"Parent":[{"id":1}],"Child":[{"id":1},{"id":2}]}"#)).unwrap();
    let baseline = store.snapshot().size();
    let none = object(r#"{"limit":0}"#);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(
        tx.relate_selected(a, &empty(), "bs", &none, true)
            .unwrap()
            .len(),
        2
    );
    assert!(
        tx.relate_selected(a, &none, "bs", &empty(), true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        tx.relate_selected(a, &empty(), "bs", &empty(), true)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        tx.relate_selected(b, &empty(), "as", &empty(), true)
            .unwrap()
            .len(),
        2
    );
    store.commit(tx.prepare().unwrap()).unwrap();
    assert!(store.snapshot().size().1 > baseline.1);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.relate_selected(b, &empty(), "as", &empty(), false)
        .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    assert_eq!(store.snapshot().size(), baseline);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(
        tx.relate_selected(parent, &empty(), "child", &empty(), true)
            .is_err()
    );
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size(), baseline);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert!(tx.relate_selected(a, &none, "id", &none, true).is_err());
    assert!(tx.prepare().is_err());
}
