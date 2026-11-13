//! Reject contradictory supplied composite-FK components even for empty selections while preserving partial-patch semantics.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, Store};
use crate::ir::MockIr;

fn object(source: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(source).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

const SDL: &str = r#"
type Target { tenant: String! code: Int! }
type Holder {
  tag: Int!
  peers: [Target] @relation(fields:["tenants","codes"], references:["tenant","code"])
    @constraint(minItems: 1, maxItems: 2)
  peer: Target @relation(fields:["tenant","code"], references:["tenant","code"])
}
type Audit { value: Int! }
type Query { holders: [Holder!]! }
"#;

fn store() -> Store {
    let ir = Arc::new(MockIr::parse(vec![Source::new("patch.graphql", SDL)]).unwrap());
    Store::seeded(
        ir,
        &object(
            r#"{
      "Target":[{"tenant":"a","code":1},{"tenant":"b","code":1}],
      "Holder":[{"tag":1,"peers":[{"tenant":"a","code":1}],"peer":{"tenant":"a","code":1}}],
      "Audit":[]
    }"#,
        ),
    )
    .unwrap()
}

#[test]
fn sd_patch_zero_matches_rejects_contradictory_composite_components_and_rolls_back() {
    let store = store();
    let snapshot = store.snapshot();
    let ir = snapshot.ir();
    let holder = ir.model_for(ir.graph().type_id("Holder").unwrap()).unwrap();
    let audit = ir.model_for(ir.graph().type_id("Audit").unwrap()).unwrap();
    for patch in [
        r#"{"tenants":null,"codes":[1]}"#,
        r#"{"tenants":["a"],"codes":null}"#,
        r#"{"tenants":["a"],"codes":[1,1]}"#,
        r#"{"tenants":[null],"codes":[1]}"#,
        r#"{"tenants":["a",null],"codes":[null,1]}"#,
        r#"{"tenant":null,"code":1}"#,
        r#"{"tenant":"a","code":null}"#,
        r#"{"codes":[]}"#,
        r#"{"tenants":null,"codes":[1,1,1]}"#,
    ] {
        for options in [object(r#"{"limit":0}"#), Object::new()] {
            let mut tx = store.begin(CallScope::MutationRoot).unwrap();
            tx.create(audit, &object(r#"{"value":1}"#)).unwrap();
            assert!(
                tx.update(holder, &options, &object(patch)).is_err(),
                "{patch}"
            );
            assert!(tx.find(audit, &Object::new()).is_err());
            assert!(tx.prepare().is_err());
            assert_eq!(store.snapshot().size(), snapshot.size());
        }
    }
}

#[test]
fn sd_patch_partial_components_merge_with_stored_values_and_zero_selection_stays_valid() {
    for patch in [
        r#"{"tenants":["b"]}"#,
        r#"{"codes":[1]}"#,
        r#"{"tenant":"b"}"#,
        r#"{"tenants":null,"codes":null,"tenant":null,"code":null}"#,
        r#"{"tenants":[null],"codes":[null]}"#,
    ] {
        let mut store = store();
        let snapshot = store.snapshot();
        let ir = snapshot.ir();
        let holder = ir.model_for(ir.graph().type_id("Holder").unwrap()).unwrap();
        let patch = object(patch);
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert!(
            tx.update(holder, &object(r#"{"limit":0}"#), &patch)
                .unwrap()
                .is_empty()
        );
        assert_eq!(tx.update(holder, &Object::new(), &patch).unwrap().len(), 1);
        let after = store.commit(tx.prepare().unwrap()).unwrap();
        let row = after.find(holder, &Object::new()).unwrap().remove(0);
        for (name, expected) in &patch {
            assert_eq!(row.get(ir.models()[holder.0].slots[name]), Some(expected));
        }
        assert_eq!(
            snapshot.find(holder, &Object::new()).unwrap()[0]
                .get(ir.models()[holder.0].slots["tag"]),
            Some(&Value::Int(1))
        );
    }
}
