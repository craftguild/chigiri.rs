//! Test model identity and snapshots used by DTO and abstract output validation.

use crate::store::test_selection::id as select;

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{ObjectData, OutputValue};
use crate::ir::{FieldId, MockIr, ModelId};
use crate::store::{CallScope, Store};

pub(super) const SDL: &str = r#"
interface Node { id: ID! }
scalar Json @representation(type: JSON)
scalar Count @representation(type: INTEGER, min: 0)
type Unit implements Node {
  id: ID!
  name: String!
  note: String
  data: Json
  next: Unit @relation(fields: ["nextId"], references: ["id"])
  payload: Payload
  computed: String! @virtual
}
type Payload implements Node {
  id: ID!
  unit: Unit
  data: Json
  count: Count @constraint(max: 10)
  child: Payload
  computed: String! @virtual
}
type Elsewhere { value: Int }
union Result = Unit | Payload
type Query {
  unit: Unit
  payload: Payload!
  result: Result
  node: Node
  units: [Unit]! @constraint(minItems: 1)
  strict: [Unit!]!
}
"#;

pub(super) fn store() -> Store {
    let ir = Arc::new(MockIr::parse(vec![Source::new("output.graphql", SDL)]).unwrap());
    Store::from_seed_json(
        ir,
        r#"{"Unit":[
        {"id":"u","name":"original","note":null,"data":{"b":2,"a":1}},
        {"id":"v","name":"second","nextId":"u"}
    ],"Payload":[{"id":"p","count":10}]}"#,
    )
    .unwrap()
}
pub(super) fn field(ir: &MockIr, parent: &str, name: &str) -> FieldId {
    ir.graph().types()[ir.graph().type_id(parent).unwrap().0].fields[name]
}
pub(super) fn model(ir: &MockIr) -> ModelId {
    ir.model_for(ir.graph().type_id("Unit").unwrap()).unwrap()
}
pub(super) fn value(json: &str) -> Value {
    chigiri_http::value_from_json(serde_json::from_str(json).unwrap()).unwrap()
}
pub(super) fn record() -> Value {
    let mut record = value(r#"{"data":{"a":1,"b":2},"note":null,"name":"original","id":"u"}"#);
    let Value::Object(object) = &mut record else {
        unreachable!()
    };
    let Some(Value::Object(data)) = object.get_mut("data") else {
        unreachable!()
    };
    data.insert("a", Value::Float(1.0));
    record
}

#[test]
fn rsm_14_output_record_matches_exact_storage_and_ignores_object_order() {
    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Query", "unit");
    let OutputValue::Object(output) = snapshot.reader().output(field, &record()).unwrap() else {
        panic!("model object required");
    };
    let ObjectData::Model(record) = output.data() else {
        panic!("model record required");
    };
    assert!(Arc::ptr_eq(
        record,
        snapshot.record(model(snapshot.ir()), "u").unwrap()
    ));
    assert!(Arc::ptr_eq(output.snapshot().ir(), snapshot.ir()));
    // Omitting a nullable field is equivalent to the normalized null stored in the record.
    assert!(
        snapshot
            .reader()
            .output(
                field,
                &value(r#"{"id":"u","name":"original","data":{"a":1,"b":2}}"#)
            )
            .is_ok()
    );
    for json in [
        r#"{"id":"u","name":"changed","note":null,"data":{"a":1,"b":2}}"#,
        r#"{"id":"fake","name":"original","note":null,"data":{"a":1,"b":2}}"#,
        r#"{"id":"u","name":"original","note":null,"data":{"a":1,"b":2},"extra":1}"#,
        r#"{"id":"v","name":"second","next":{"id":"u"}}"#,
        r#"{"id":"v","name":"second","computed":"forged"}"#,
        r#"{"id":"v","name":"second","nextId":"v"}"#,
    ] {
        assert!(
            snapshot.reader().output(field, &value(json)).is_err(),
            "{json}"
        );
    }
}

#[test]
fn rsm_07_output_embedded_dto_projects_values_and_validates_nested_objects() {
    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Unit", "payload");
    let mut payload = Object::new();
    payload.insert("id", "payload");
    payload.insert("unit", record());
    payload.insert("data", value(r#"{"nested":[null,{},[]]}"#));
    let OutputValue::Object(output) = snapshot
        .reader()
        .output(field, &Value::Object(payload))
        .unwrap()
    else {
        panic!("DTO required");
    };
    let ObjectData::Dto(fields) = output.data() else {
        panic!("DTO data required");
    };
    assert_eq!(fields.len(), 3);
    // An embedded DTO may contain new scalar values even though its model field must reference this snapshot.
    assert!(
        snapshot
            .reader()
            .output(
                field,
                &value(r#"{"id":"new-payload","unit":{"id":"new-unit","name":"embedded"}}"#)
            )
            .is_ok()
    );
    for json in [
        r#"{}"#,
        r#"{"id":"payload","computed":"forged"}"#,
        r#"{"id":"payload","unknown":null}"#,
        r#"{"id":"payload","unit":{"id":"fake"}}"#,
        r#"{"id":"payload","child":{}}"#,
        r#"{"id":"payload","count":11}"#,
        r#"{"id":"payload","count":-1}"#,
        r#"{"id":"payload","count":1.5}"#,
    ] {
        assert!(
            snapshot.reader().output(field, &value(json)).is_err(),
            "{json}"
        );
    }
}

#[test]
fn sd_root_identity_output_still_requires_a_stored_copy() {
    let store = store();
    let snapshot = store.snapshot();
    let field = field(snapshot.ir(), "Query", "payload");
    assert!(
        snapshot
            .reader()
            .output(field, &value(r#"{"id":"p","count":10}"#))
            .is_ok()
    );
    for raw in [
        Value::Null,
        value(r#"{"id":"new-payload"}"#),
        value(r#"{"id":"p","count":9}"#),
    ] {
        assert!(snapshot.reader().output(field, &raw).is_err());
    }
}

#[test]
fn rsm_07_abstract_requires_a_known_concrete_member_and_checks_its_payload() {
    let store = store();
    let snapshot = store.snapshot();
    for name in ["result", "node"] {
        let field = field(snapshot.ir(), "Query", name);
        for json in [
            r#"{"__typename":"Unit","id":"v","name":"second","nextId":"u"}"#,
            r#"{"__typename":"Payload","id":"p","count":10}"#,
        ] {
            let OutputValue::Object(output) =
                snapshot.reader().output(field, &value(json)).unwrap()
            else {
                panic!("concrete object required");
            };
            assert_eq!(
                snapshot.ir().graph().types()[output.ty().0].kind,
                chigiri::core::schema::TypeKind::Object
            );
        }
        for json in [
            r#"{"id":"p"}"#,
            r#"{"__typename":"NoSuchType","id":"p"}"#,
            r#"{"__typename":"Node","id":"p"}"#,
            r#"{"__typename":"Elsewhere"}"#,
            r#"{"__typename":null,"id":"p"}"#,
            r#"{"__typename":"Payload","count":1}"#,
            r#"{"__typename":"Unit","id":"v","name":"forged","nextId":"u"}"#,
        ] {
            assert!(
                snapshot.reader().output(field, &value(json)).is_err(),
                "{json}"
            );
        }
    }
    let concrete = field(snapshot.ir(), "Query", "unit");
    assert!(
        snapshot
            .reader()
            .output(concrete, &value(r#"{"__typename":"Payload","id":"p"}"#))
            .is_err()
    );
}

#[test]
fn rsm_14_return_list_keeps_nullable_items_and_outer_constraints() {
    let store = store();
    let snapshot = store.snapshot();
    let nullable = field(snapshot.ir(), "Query", "units");
    let strict = field(snapshot.ir(), "Query", "strict");
    let list = Value::List(vec![record(), Value::Null]);
    let OutputValue::List(values) = snapshot.reader().output(nullable, &list).unwrap() else {
        panic!("list required");
    };
    assert!(matches!(values[1], OutputValue::Null));
    assert!(snapshot.reader().output(strict, &list).is_err());
    assert!(
        snapshot
            .reader()
            .output(nullable, &Value::List(vec![]))
            .is_err()
    );
    assert!(snapshot.reader().output(nullable, &Value::Null).is_err());
}

#[test]
fn rsm_12_invalid_return_prevents_all_model_commit_and_registration_is_preserved() {
    let mut store = store();
    let snapshot = store.snapshot();
    let model = model(snapshot.ir());
    let field = field(snapshot.ir(), "Query", "unit");
    let input = [
        ("id", Value::from("Unit-1")),
        ("name", Value::from("created")),
    ]
    .into_iter()
    .collect();
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.create(model, &input).unwrap().row(), 2);
    let mut prepared = tx.prepare().unwrap();
    assert!(
        prepared
            .output(field, &value(r#"{"id":"Unit-1","name":"forged"}"#))
            .is_err()
    );
    assert!(prepared.output(field, &record()).is_err());
    assert!(store.commit(prepared).is_err());
    assert_eq!(store.snapshot().size().0, 3);
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    assert_eq!(tx.create(model, &input).unwrap().row(), 2);
    let mut prepared = tx.prepare().unwrap();
    assert!(
        prepared
            .output(field, &value(r#"{"id":"Unit-1","name":"created"}"#))
            .is_ok()
    );
    assert_eq!(store.commit(prepared).unwrap().size().0, 4);
}

#[test]
fn rsm_13_output_and_reader_keep_original_snapshot_after_later_commit() {
    let mut store = store();
    let snapshot = store.snapshot();
    let model = model(snapshot.ir());
    let field = field(snapshot.ir(), "Query", "unit");
    let mut reader = snapshot.reader();
    let OutputValue::Object(output) = reader.output(field, &record()).unwrap() else {
        panic!("object required");
    };
    let mut tx = store.begin(CallScope::MutationRoot).unwrap();
    tx.update(
        model,
        &select("u"),
        &[("name", Value::from("later"))].into_iter().collect(),
    )
    .unwrap();
    store.commit(tx.prepare().unwrap()).unwrap();
    assert!(reader.output(field, &record()).is_ok());
    assert!(output.snapshot().reader().output(field, &record()).is_ok());
    assert!(store.snapshot().reader().output(field, &record()).is_err());
    reader.fail();
    assert!(reader.output(field, &record()).is_err());
    assert!(reader.find(model, &Object::new()).is_err());
}
