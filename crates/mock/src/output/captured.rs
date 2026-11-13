//! Seal captured record slots without rebinding them to a current store row.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::Value;

use super::{ObjectData, OutputObject, OutputValue};
use crate::ir::{MockIr, ModelId, SlotId, TypeId};
use crate::store::Snapshot;
use crate::value::ValueError;

/// Canonical publication-time record slots authorized for fresh relation lookup.
#[derive(Debug)]
#[doc(hidden)]
pub struct CapturedRecord {
    scope: Arc<()>,
    model: ModelId,
    values: BTreeMap<SlotId, Value>,
    identity: Option<Box<str>>,
}

impl CapturedRecord {
    pub(crate) fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    pub(crate) fn belongs_to(&self, ir: &MockIr) -> bool {
        Arc::ptr_eq(&self.scope, ir.scope())
    }

    pub(crate) const fn model(&self) -> ModelId {
        self.model
    }

    pub(crate) fn get(&self, slot: SlotId) -> Option<&Value> {
        self.values.get(&slot)
    }

    pub(crate) fn values(&self) -> impl ExactSizeIterator<Item = (SlotId, &Value)> {
        self.values.iter().map(|(slot, value)| (*slot, value))
    }
}

pub(super) fn record(
    snapshot: Snapshot,
    ty: TypeId,
    model: ModelId,
    values: BTreeMap<SlotId, Value>,
    identity: Option<Box<str>>,
) -> Result<OutputValue, ValueError> {
    let scope = snapshot.ir().scope().clone();
    let ir = snapshot.ir();
    let definition = ir
        .models()
        .get(model.0)
        .ok_or(ValueError("unknown captured record model"))?;
    if definition.ty != ty
        || values.len() != definition.slots.len()
        || !definition
            .slots
            .values()
            .all(|slot| values.contains_key(slot))
    {
        return Err(ValueError("invalid captured record shape"));
    }
    Ok(OutputValue::Object(OutputObject {
        snapshot,
        ty,
        data: ObjectData::CapturedRecord(Arc::new(CapturedRecord {
            scope,
            model,
            values,
            identity,
        })),
    }))
}

pub(super) fn fields(
    snapshot: Snapshot,
    ty: TypeId,
    values: BTreeMap<crate::ir::FieldId, OutputValue>,
    stored: bool,
) -> Result<OutputValue, ValueError> {
    if snapshot.ir().graph().types().get(ty.0).is_none() {
        return Err(ValueError("unknown captured object type"));
    }
    let data = if stored {
        ObjectData::Stored(Arc::new(values))
    } else {
        ObjectData::Dto(Arc::new(values))
    };
    Ok(OutputValue::Object(OutputObject { snapshot, ty, data }))
}
