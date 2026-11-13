//! Group adopted subscription destinations by their exact SDL payload type name.

use std::collections::BTreeMap;

use crate::ir::{FieldId, MockIr, TypeId};

pub(crate) struct EventType {
    pub(crate) named: TypeId,
    pub(crate) fields: Vec<FieldId>,
}

pub(crate) fn event_types(ir: &MockIr) -> Vec<EventType> {
    let mut types = BTreeMap::<TypeId, Vec<FieldId>>::new();
    for script in ir
        .scripts()
        .iter()
        .filter(|script| script.role.is_subscription_root())
    {
        types
            .entry(ir.graph().fields()[script.field.0].ty.named)
            .or_default()
            .push(script.field);
    }
    types
        .into_iter()
        .map(|(named, fields)| EventType { named, fields })
        .collect()
}

pub(crate) fn field_type_name(ir: &MockIr, field: FieldId) -> &str {
    &ir.graph().types()[ir.graph().fields()[field.0].ty.named.0].name
}
