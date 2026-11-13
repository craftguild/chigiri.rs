use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use crate::ir::{FieldId, MockIr, ModelId, SlotId};
use crate::output::{ObjectData, OutputValue};
use crate::publication::PublicationLimits;
use crate::publication::batch::{BatchBuilder, CapturedBatch};
use crate::store::{Record, Snapshot, Store};
use crate::value::ValueBudget;

pub(super) const SDL: &str = r#"
schema { query: Query subscription: Subscription }
scalar Json @representation(type: JSON)
type User {
  tenant: ID!
  code: Int!
  name: String!
  posts: [Post!]! @relation
}
type Post {
  title: String!
  author: User @relation(fields: ["tenantFK", "codeFK"], references: ["tenant", "code"])
}
type A { id: Int! name: String! bs: [B!]! @relation }
type B { id: Boolean! label: String! as: [A!]! @relation }
type Envelope { value: String! }
type Query { users: [User!]! }
type Subscription {
  post: Post!
  user: User!
  a: A!
  b: B!
  dto: Envelope!
  stored: Envelope!
  text: String!
  json: Json!
}
"#;

pub(super) struct Fixture {
    pub(super) ir: Arc<MockIr>,
    pub(super) user: ModelId,
    pub(super) post: ModelId,
    pub(super) a: ModelId,
    pub(super) b: ModelId,
    pub(super) post_event: FieldId,
    pub(super) user_event: FieldId,
    pub(super) a_event: FieldId,
    pub(super) b_event: FieldId,
    pub(super) dto: FieldId,
    pub(super) stored: FieldId,
    pub(super) text: FieldId,
    pub(super) json: FieldId,
}

pub(super) fn fixture() -> Fixture {
    let ir = Arc::new(MockIr::parse(vec![Source::new("projection.graphql", SDL)]).unwrap());
    let subscription = ir.graph().type_id("Subscription").unwrap();
    let fields = &ir.graph().types()[subscription.0].fields;
    Fixture {
        user: model(&ir, "User"),
        post: model(&ir, "Post"),
        a: model(&ir, "A"),
        b: model(&ir, "B"),
        post_event: fields["post"],
        user_event: fields["user"],
        a_event: fields["a"],
        b_event: fields["b"],
        dto: fields["dto"],
        stored: fields["stored"],
        text: fields["text"],
        json: fields["json"],
        ir,
    }
}

pub(super) fn seeded(fixture: &Fixture) -> Store {
    Store::seeded(
        fixture.ir.clone(),
        &json(
            r#"{
              "User":[{"tenant":"t","code":1,"name":"captured-user"}],
              "Post":[{"title":"captured-post","tenantFK":"t","codeFK":1}],
              "A":[{"id":1,"name":"captured-a"}],
              "B":[{"id":true,"label":"captured-b"}]
            }"#,
        ),
    )
    .unwrap()
}

pub(super) fn capture(fixture: &Fixture, field: FieldId, output: &OutputValue) -> CapturedBatch {
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder.push(&fixture.ir, field, "topic", output).unwrap();
    builder.seal().unwrap()
}

pub(super) fn project(
    batch: &CapturedBatch,
    snapshot: Snapshot,
    field: FieldId,
    budget: &mut ValueBudget,
) -> OutputValue {
    batch.publications()[0]
        .project(snapshot, field, budget)
        .unwrap()
}

pub(super) fn record_output(
    snapshot: &Snapshot,
    field: FieldId,
    model: ModelId,
    position: usize,
) -> OutputValue {
    let row = snapshot.find(model, &Object::new()).unwrap()[position].clone();
    let raw = row_object(snapshot.ir(), &row);
    snapshot
        .reader()
        .output(field, &Value::Object(raw))
        .unwrap()
}

pub(super) fn captured_record(output: &OutputValue) -> &crate::output::CapturedRecord {
    let OutputValue::Object(object) = output else {
        panic!("captured object required")
    };
    let ObjectData::CapturedRecord(record) = object.data() else {
        panic!("captured record required")
    };
    record
}

pub(super) fn slot(fixture: &Fixture, model: ModelId, name: &str) -> SlotId {
    fixture.ir.models()[model.0].slots[name]
}

pub(super) fn row_object(ir: &MockIr, row: &Record) -> Object {
    row.values()
        .map(|(slot, value)| (ir.slots()[slot.0].name.to_string(), value.clone()))
        .collect()
}

pub(super) fn selection(values: &[(&str, Value)]) -> Object {
    let all = values
        .iter()
        .map(|(field, value)| {
            Value::Object(Object::from_iter([
                ("field", Value::from(*field)),
                ("op", Value::from("eq")),
                ("value", value.clone()),
            ]))
        })
        .collect();
    Object::from_iter([(
        "where",
        Value::Object(Object::from_iter([("all", Value::List(all))])),
    )])
}

pub(super) fn json(source: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(source).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn model(ir: &MockIr, name: &str) -> ModelId {
    ir.model_for(ir.graph().type_id(name).unwrap()).unwrap()
}
