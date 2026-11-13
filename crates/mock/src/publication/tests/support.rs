//! Shared schema and immutable batch fixtures.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::super::batch::{BatchBuilder, CapturedBatch};
use super::super::{PublicationLimits, limits};
use crate::ir::{FieldId, MockIr, ModelId};
use crate::output::OutputValue;
use crate::store::{CallScope, Store};

const SDL: &str = r#"
schema { query: Query subscription: Subscription }
scalar Json @representation(type: JSON)
type Unit { id: ID! name: String! data: Json }
type Envelope { value: String! }
type Query { unit: Unit }
type Subscription { event: Json changed: Unit dto: Envelope stored: Envelope }
"#;

pub(super) struct Fixture {
    pub(super) ir: Arc<MockIr>,
    pub(super) query_unit: FieldId,
    pub(super) event: FieldId,
    pub(super) changed: FieldId,
    pub(super) dto: FieldId,
    pub(super) stored: FieldId,
    pub(super) unit: ModelId,
}

pub(super) fn fixture() -> Fixture {
    let ir = Arc::new(MockIr::parse(vec![Source::new("publication.graphql", SDL)]).unwrap());
    let subscription = ir.graph().type_id("Subscription").unwrap();
    let query = ir.graph().type_id("Query").unwrap();
    let unit_type = ir.graph().type_id("Unit").unwrap();
    Fixture {
        query_unit: ir.graph().types()[query.0].fields["unit"],
        event: ir.graph().types()[subscription.0].fields["event"],
        changed: ir.graph().types()[subscription.0].fields["changed"],
        dto: ir.graph().types()[subscription.0].fields["dto"],
        stored: ir.graph().types()[subscription.0].fields["stored"],
        unit: ir.model_for(unit_type).unwrap(),
        ir,
    }
}

pub(super) fn object_output(fixture: &Fixture, field: FieldId, stored: bool) -> OutputValue {
    let snapshot = Store::empty(Arc::clone(&fixture.ir)).snapshot();
    let mut value = Object::new();
    value.insert("value", "same");
    let mut reader = snapshot.reader();
    if stored {
        reader.stored_output(field, &Value::Object(value)).unwrap()
    } else {
        reader.output(field, &Value::Object(value)).unwrap()
    }
}

pub(super) fn scalar(value: impl Into<Value>) -> OutputValue {
    OutputValue::Scalar(value.into())
}

pub(super) fn batch(fixture: &Fixture, topic: &str, payload: &OutputValue) -> CapturedBatch {
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.event, topic, payload)
        .unwrap();
    builder.seal().unwrap()
}

pub(super) fn record_output(fixture: &Fixture, name: &str) -> (Store, OutputValue) {
    let mut seed = Object::new();
    seed.insert("Unit", Value::List(vec![Value::Object(unit("u", name))]));
    let store = Store::seeded(Arc::clone(&fixture.ir), &seed).unwrap();
    let snapshot = store.snapshot();
    let query = fixture.ir.graph().type_id("Query").unwrap();
    let field = fixture.ir.graph().types()[query.0].fields["unit"];
    let output = snapshot
        .reader()
        .output(field, &Value::Object(unit("u", name)))
        .unwrap();
    (store, output)
}

pub(super) fn replace_record(store: &mut Store, fixture: &Fixture) {
    let mut update = store.begin(CallScope::MutationRoot).unwrap();
    let mut patch = Object::new();
    patch.insert("name", "intermediate");
    let mut identity = Object::new();
    identity.insert("where", selection("u"));
    update.update(fixture.unit, &identity, &patch).unwrap();
    store.commit(update.prepare().unwrap()).unwrap();

    let mut transaction = store.begin(CallScope::MutationRoot).unwrap();
    let mut identity = Object::new();
    identity.insert("where", selection("u"));
    transaction.delete(fixture.unit, &identity).unwrap();
    transaction
        .create(fixture.unit, &unit("u", "replacement"))
        .unwrap();
    store.commit(transaction.prepare().unwrap()).unwrap();
}

fn selection(id: &str) -> Value {
    let mut selection = Object::new();
    selection.insert("field", "id");
    selection.insert("op", "eq");
    selection.insert("value", id);
    Value::Object(selection)
}

fn unit(id: &str, name: &str) -> Object {
    let mut unit = Object::new();
    unit.insert("id", id);
    unit.insert("name", name);
    unit.insert("data", Value::Null);
    unit
}

pub(super) fn exact_limits(bytes: usize) -> PublicationLimits {
    PublicationLimits::new(limits::MAX_PUBLICATIONS, bytes).unwrap()
}
