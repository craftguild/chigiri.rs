//! Register and share non-null scalar slot groups used as identities and reference keys.

use chigiri::core::schema::TypeKind;

use super::Build;
use crate::ir::{FieldId, FieldPlan, KeyId, KeySpec, ModelId, SlotId};
use crate::schema::SchemaError;

pub(super) fn register(
    build: &mut Build,
    field: FieldId,
    model: ModelId,
    names: &[std::sync::Arc<str>],
) -> Result<KeyId, SchemaError> {
    let mut slots = Vec::new();
    for name in names {
        let slot = build.models[model.0]
            .slots
            .get(name)
            .copied()
            .ok_or_else(|| build.error(field, "reference key requires a stored field"))?;
        validate(build, field, slot)?;
        slots.push(slot);
    }
    if let Some(index) = build
        .keys
        .iter()
        .position(|key| key.model == model && key.slots == slots)
    {
        return Ok(KeyId(index));
    }
    build.models[model.0]
        .immutable_slots
        .extend(slots.iter().copied());
    let id = KeyId(build.keys.len());
    build.keys.push(KeySpec { model, slots });
    Ok(id)
}

pub(super) fn identity(
    build: &mut Build,
    field: FieldId,
    model: ModelId,
) -> Result<KeyId, SchemaError> {
    let key = build.models[model.0]
        .key
        .ok_or_else(|| build.error(field, "implicit relation requires non-null identity"))?;
    let name = build.graph.fields()[key.0].name.clone();
    register(build, field, model, &[name])
}

fn validate(build: &Build, field: FieldId, slot: SlotId) -> Result<(), SchemaError> {
    let node = &build.slots[slot.0];
    let named = &build.graph.types()[node.ty.named.0];
    if node.ty.nullable()
        || node.ty.list_depth() != 0
        || !(named.kind == TypeKind::Enum
            || matches!(named.name.as_ref(), "ID" | "String" | "Int" | "Boolean"))
        || !matches!(build.plans[node.field.0], Some(FieldPlan::Stored(_)))
    {
        return Err(build.error(field, "reference key must be a non-null stored key scalar"));
    }
    if matches!(named.name.as_ref(), "ID" | "String") {
        let field = &build.graph.fields()[node.field.0];
        crate::ir::constraints::validate_key(&node.constraints, field.span)?;
    }
    Ok(())
}
