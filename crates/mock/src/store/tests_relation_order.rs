//! Preserve selection order on both relation sides across no-op, disconnect, reconnect, and rollback paths.

use chigiri::core::Object;

use super::test_selection::id as select;
use super::tests::{ir, model};
use super::{CallScope, Related, Snapshot, Store};

fn options(json: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(json).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn related(snapshot: &Snapshot, ty: &str, field: &str, id: &str) -> Vec<String> {
    let ir = snapshot.ir();
    let model = model(ir, ty);
    let field = ir.graph().types()[ir.models()[model.0].ty.0].fields[field];
    let Related::Many(rows) = snapshot
        .related(field, snapshot.record(model, id).unwrap())
        .unwrap()
    else {
        panic!("relation list");
    };
    rows.into_iter()
        .map(|row| row.unwrap().id().unwrap().to_owned())
        .collect()
}

#[test]
fn sd_11_selected_edge_order_survives_noop_disconnect_reconnect_and_rollback() {
    for explicit in [false, true] {
        let owner = if explicit {
            "@relation(fields:[\"bIds\"],references:[\"id\"])"
        } else {
            "@relation"
        };
        let ir = ir(&format!(
            "type A{{id:ID! bs:[B!]! {owner}}} type B{{id:ID! as:[A!]! @relation}} type Query{{a:[A!]!}}"
        ));
        let mut store = Store::from_seed_json(ir.clone(), if explicit {
            r#"{"A":[{"id":"a1","bIds":[]},{"id":"a2","bIds":[]}],"B":[{"id":"b1"},{"id":"b2"},{"id":"b3"}]}"#
        } else {
            r#"{"A":[{"id":"a1"},{"id":"a2"}],"B":[{"id":"b1"},{"id":"b2"},{"id":"b3"}]}"#
        }).unwrap();
        let a = model(&ir, "A");
        let b = model(&ir, "B");
        let descending = options(r#"{"orderBy":[{"field":"id","direction":"desc"}]}"#);
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.connect(a, &descending, "bs", &descending).unwrap();
        store.commit(tx.prepare().unwrap()).unwrap();
        let original = store.snapshot();
        assert_eq!(related(&original, "A", "bs", "a1"), ["b3", "b2", "b1"]);
        assert_eq!(related(&original, "B", "as", "b1"), ["a2", "a1"]);
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.connect(a, &Object::new(), "bs", &Object::new()).unwrap();
        tx.disconnect(b, &select("b2"), "as", &select("a1"))
            .unwrap();
        store.commit(tx.prepare().unwrap()).unwrap();
        assert_eq!(related(&store.snapshot(), "A", "bs", "a1"), ["b3", "b1"]);
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.connect(b, &select("b2"), "as", &select("a1")).unwrap();
        store.commit(tx.prepare().unwrap()).unwrap();
        let before = store.snapshot();
        assert_eq!(related(&before, "A", "bs", "a1"), ["b3", "b1", "b2"]);
        assert_eq!(related(&before, "B", "as", "b2"), ["a2", "a1"]);
        let sequence = before.state.next_edge;
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        tx.disconnect(a, &select("a1"), "bs", &select("b3"))
            .unwrap();
        tx.connect(a, &select("a1"), "bs", &select("b3")).unwrap();
        assert!(
            tx.connect(a, &Object::new(), "missing", &Object::new())
                .is_err()
        );
        assert!(tx.prepare().is_err());
        assert_eq!(store.snapshot().state.next_edge, sequence);
        assert_eq!(
            related(&store.snapshot(), "A", "bs", "a1"),
            ["b3", "b1", "b2"]
        );
        assert_eq!(related(&original, "A", "bs", "a1"), ["b3", "b2", "b1"]);
    }
}
