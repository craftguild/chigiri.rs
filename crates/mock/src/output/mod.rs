//! Bind each validated return value and type ID to the same immutable snapshot.

mod captured;
mod failure;
pub(crate) mod related;
mod stored;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_authoring;
#[cfg(test)]
mod tests_limits;
mod validate;

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::Value;

use crate::codec::Provenance;
use crate::ir::{FieldId, ModelId, SlotId, TypeId};
use crate::store::{Record, Snapshot};
use crate::value::{ValueBudget, ValueError};

pub(crate) use failure::OutputError;

/// Validated resolver result whose objects all share one immutable snapshot.
#[derive(Clone)]
pub enum OutputValue {
    /// Null value.
    Null,
    /// Scalar or enum value, including a custom scalar represented by a JSON container.
    Scalar(Value),
    /// List that preserves explicit null items.
    List(Vec<OutputValue>),
    /// Concrete model or DTO.
    Object(OutputObject),
}

/// Backing data for a model record, host-owned stored value, or resolver DTO.
#[derive(Clone)]
pub enum ObjectData {
    /// Immutable record bound to the output snapshot.
    Model(Arc<Record>),
    /// Immutable captured record fields with relation authority but no current row identity.
    CapturedRecord(Arc<CapturedRecord>),
    /// Host-read stored value without independent record identity or relations.
    Stored(Arc<BTreeMap<FieldId, OutputValue>>),
    /// Validated DTO field values keyed by resolved field ID.
    Dto(Arc<BTreeMap<FieldId, OutputValue>>),
}

pub(crate) use captured::CapturedRecord;

pub(crate) fn captured_record(
    snapshot: Snapshot,
    ty: TypeId,
    model: ModelId,
    values: BTreeMap<SlotId, Value>,
    identity: Option<Box<str>>,
) -> Result<OutputValue, ValueError> {
    captured::record(snapshot, ty, model, values, identity)
}

pub(crate) fn captured_fields(
    snapshot: Snapshot,
    ty: TypeId,
    values: BTreeMap<FieldId, OutputValue>,
    stored: bool,
) -> Result<OutputValue, ValueError> {
    captured::fields(snapshot, ty, values, stored)
}

/// Object whose type ID remains bound to its schema instance and immutable snapshot.
#[derive(Clone)]
pub struct OutputObject {
    snapshot: Snapshot,
    ty: TypeId,
    data: ObjectData,
}

impl OutputObject {
    /// Snapshot read by subsequent nested resolvers.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    /// Concrete type selected for abstract output.
    pub fn ty(&self) -> TypeId {
        self.ty
    }
    /// Return the verified unchanged record or validated DTO data.
    pub fn data(&self) -> &ObjectData {
        &self.data
    }
}

pub(crate) fn validate(
    snapshot: &Snapshot,
    field: FieldId,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<OutputValue, ValueError> {
    validate_detailed(snapshot, field, value, budget).map_err(OutputError::into_value)
}

pub(crate) fn validate_detailed(
    snapshot: &Snapshot,
    field: FieldId,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<OutputValue, OutputError> {
    let definition = snapshot
        .ir()
        .graph()
        .fields()
        .get(field.0)
        .ok_or(ValueError("unknown output field"))?;
    let mut validator = validate::Validator::new(snapshot, budget, None);
    let result = validator.field(definition, value, 1);
    let member = validator.failure_member.unwrap_or(field);
    let resource = validator.budget.work.failure().is_some();
    result.map_err(|error| {
        OutputError::from(error)
            .with_member(member)
            .with_resource(resource)
    })
}

pub(crate) fn validate_provenance_detailed(
    snapshot: &Snapshot,
    field: FieldId,
    value: &Value,
    provenance: &Provenance,
    budget: &mut ValueBudget,
) -> Result<OutputValue, OutputError> {
    let definition = snapshot
        .ir()
        .graph()
        .fields()
        .get(field.0)
        .ok_or(ValueError("unknown output field"))?;
    let mut validator = validate::Validator::new(snapshot, budget, Some(provenance));
    let result = validator.field(definition, value, 1);
    let member = validator.failure_member.unwrap_or(field);
    let resource = validator.budget.work.failure().is_some();
    result.map_err(|error| {
        OutputError::from(error)
            .with_member(member)
            .with_resource(resource)
    })
}

pub(crate) fn validate_stored_detailed(
    snapshot: &Snapshot,
    field: FieldId,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<OutputValue, OutputError> {
    let definition = snapshot
        .ir()
        .graph()
        .fields()
        .get(field.0)
        .ok_or(ValueError("unknown output field"))?;
    let mut validator = validate::Validator::new(snapshot, budget, None);
    let result = validator.stored_field(definition, value, 1);
    let member = validator.failure_member.unwrap_or(field);
    let resource = validator.budget.work.failure().is_some();
    result.map_err(|error| {
        OutputError::from(error)
            .with_member(member)
            .with_resource(resource)
            .stored()
    })
}
