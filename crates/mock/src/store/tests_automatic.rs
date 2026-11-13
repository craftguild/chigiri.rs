//! Test automatic models, varied identities, bounded nesting, and incomplete seeds in a real store.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, Store};
use crate::ir::{FieldPlan, MockIr, ModelId};

fn ir(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("automatic.graphql", source)]).unwrap())
}

fn model(ir: &MockIr, name: &str) -> ModelId {
    ir.model_for(ir.graph().type_id(name).unwrap()).unwrap()
}

fn object(entries: &[(&str, Value)]) -> Object {
    entries.iter().cloned().collect()
}

#[test]
fn sd_01_concrete_objects_are_models_and_actual_operation_roots_are_excluded() {
    let ir = ir(
        "schema{query:Read mutation:Write} interface I{name:String} type Query implements I{name:String} type Payload{next:[Payload]} union U=Query|Payload type Read{x:U} type Write{x:Payload}",
    );
    let names: Vec<_> = ir
        .models()
        .iter()
        .map(|m| ir.graph().types()[m.ty.0].name.as_ref())
        .collect();
    assert_eq!(names, ["Query", "Payload"]);
    assert!(ir.models().iter().all(|m| m.key.is_none() && !m.auto_id));
    for ty in ["Read", "Write", "I", "U", "ChigiriMockRepresentation"] {
        assert!(ir.model_for(ir.graph().type_id(ty).unwrap()).is_none());
    }
    let payload = model(&ir, "Payload");
    let slot = ir.models()[payload.0].slots["next"];
    assert!(matches!(
        ir.field_plan(ir.slots()[slot.0].field),
        Some(FieldPlan::Stored(_))
    ));
}

#[test]
fn sd_02_idless_duplicates_and_mutual_lists_remain_finite_embedded_values() {
    let ir = ir(
        "type Payload{label:String! nodes:[Node!]!} type Node{values:[Payload!]!} type Query{payload:Payload}",
    );
    let payload = model(&ir, "Payload");
    let node = model(&ir, "Node");
    let input = object(&[
        ("label", "same".into()),
        (
            "nodes",
            Value::List(vec![Value::Object(object(&[(
                "values",
                Value::List(vec![]),
            )]))]),
        ),
    ]);
    let mut store = Store::empty(ir.clone());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let first = tx.create(payload, &input).unwrap();
    let second = tx.create(payload, &input).unwrap();
    assert_eq!(first.id(), None);
    assert_eq!(second.id(), None);
    assert_ne!(first.row(), second.row());
    assert_eq!(first.values().len(), 2);
    assert_eq!(tx.count(node, &Object::new()).unwrap(), 0);
    assert_eq!(tx.find(payload, &Object::new()).unwrap().len(), 2);
    let snapshot = store.commit(tx.prepare().unwrap()).unwrap();
    let field = ir.graph().types()[ir.graph().query().0].fields["payload"];
    let output = crate::output::validate(
        &snapshot,
        field,
        &Value::Object(input.clone()),
        &mut Default::default(),
    )
    .unwrap();
    assert!(matches!(output, crate::output::OutputValue::Object(_)));
    let mut changed = input;
    changed.insert("label", "unsaved value");
    let value = crate::output::validate(
        &snapshot,
        field,
        &Value::Object(changed),
        &mut Default::default(),
    )
    .unwrap();
    let crate::output::OutputValue::Object(value) = value else {
        panic!("Normal object value");
    };
    assert!(matches!(value.data(), crate::output::ObjectData::Dto(_)));
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn sd_05_seed_requires_explicit_nonnull_id_and_runtime_preserves_explicit_case() {
    let ir = ir("type Item{id:ID! value:String} type Query{items:[Item!]!}");
    let item = model(&ir, "Item");
    let seed = object(&[("Item", Value::List(vec![Value::Object(Object::new())]))]);
    assert!(Store::seeded(ir.clone(), &seed).is_err());
    let store = Store::empty(ir);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let explicit = tx
        .create(item, &object(&[("id", "MixedCASE".into())]))
        .unwrap();
    assert_eq!(explicit.id(), Some("MixedCASE"));
    let generated = tx.create(item, &Object::new()).unwrap();
    let id = generated.id().unwrap();
    assert_eq!(id.len(), 26);
    assert_eq!(id, id.to_ascii_lowercase());
    assert!(ulid::Ulid::from_string(id).is_ok());
    assert!(
        tx.create(item, &object(&[("id", "MixedCASE".into())]))
            .is_err()
    );
    assert!(tx.prepare().is_err());
    assert_eq!(store.snapshot().size().0, 0);
}

#[test]
fn sd_05_nullable_and_other_scalar_ids_are_optional_without_ulid_fallback() {
    let ir = ir(
        "enum Flag{A B} type Maybe{id:ID} type Number{id:Int!} type BooleanKey{id:Boolean!} type EnumKey{id:Flag!} type Ordinary{id:Float} type Query{x:Int}",
    );
    let store = Store::empty(ir.clone());
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    let maybe = model(&ir, "Maybe");
    assert_eq!(tx.create(maybe, &Object::new()).unwrap().id(), None);
    assert_eq!(
        tx.create(maybe, &object(&[("id", Value::Null)]))
            .unwrap()
            .id(),
        None
    );
    for (name, value) in [
        ("Number", Value::Int(7)),
        ("BooleanKey", Value::Boolean(false)),
        ("EnumKey", "A".into()),
    ] {
        let model = model(&ir, name);
        assert!(!ir.models()[model.0].auto_id);
        assert!(
            tx.create(model, &object(&[("id", value)]))
                .unwrap()
                .id()
                .is_some()
        );
    }
    let ordinary = model(&ir, "Ordinary");
    assert!(ir.models()[ordinary.0].key.is_none());
    assert_eq!(
        tx.create(ordinary, &object(&[("id", Value::Float(1.5))]))
            .unwrap()
            .id(),
        None
    );
    tx.prepare().unwrap();
    for name in ["Number", "BooleanKey", "EnumKey"] {
        let mut tx = store.begin(CallScope::MutationRoot).unwrap();
        assert!(tx.create(model(&ir, name), &Object::new()).is_err());
        assert!(tx.prepare().is_err());
    }
}
