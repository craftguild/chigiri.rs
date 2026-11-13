//! Test identity lookups and the complete closed `where` condition vocabulary.

use chigiri::core::{Object, Value};

pub(crate) fn id(value: &str) -> Object {
    Object::from_iter([(
        "where",
        Value::Object(Object::from_iter([
            ("field", Value::from("id")),
            ("op", Value::from("eq")),
            ("value", Value::from(value)),
        ])),
    )])
}

pub(crate) fn parent(
    snapshot: &super::Snapshot,
    field: crate::ir::FieldId,
    id: &str,
) -> std::sync::Arc<super::Record> {
    let ir = snapshot.ir();
    let model = ir.model_for(ir.graph().fields()[field.0].parent).unwrap();
    snapshot.record(model, id).unwrap().clone()
}
