//! Classify each concrete field as stored data, DTO data, a relation, or a script.

use std::sync::Arc;

use chigiri::core::schema::TypeKind;

use super::{
    Build,
    directives::{self, Role},
};
use crate::ir::{FieldId, FieldPlan, ModelId, Slot, SlotId, TypeRef};
use crate::schema::SchemaError;

pub(super) fn classify(build: &mut Build) -> Result<(), SchemaError> {
    for index in 0..build.graph.fields().len() {
        let id = FieldId(index);
        let field = &build.graph.fields()[index];
        let role = directives::role(&build.graph.field_source(id).unwrap().directives)?;
        let root = build.graph.is_operation_root(field.parent);
        let model = build.model_types.get(&field.parent).copied();
        if build.graph.types()[field.parent.0].kind == TypeKind::Interface {
            if matches!(role, Role::Relation) {
                return Err(build.error(id, "concrete field role required"));
            }
            continue;
        }
        if role == Role::Manual && !field.arguments.is_empty() {
            return Err(build.error(id, "virtual requires an argumentless field"));
        }
        if root {
            if role == Role::Relation {
                return Err(build.error(id, "relation requires a stored model"));
            }
            continue;
        }
        match role {
            Role::Manual => {}
            Role::Relation => {}
            Role::Plain => {
                if !field.arguments.is_empty() {
                    continue;
                }
                if let Some(model) = model {
                    let slot = add_slot(build, id, model, field.name.clone(), field.ty.clone())?;
                    build.plans[index] = Some(FieldPlan::Stored(slot));
                } else {
                    build.plans[index] = Some(FieldPlan::Value);
                }
            }
        }
    }
    Ok(())
}

pub(super) fn add_slot(
    build: &mut Build,
    field: FieldId,
    model: ModelId,
    name: Arc<str>,
    ty: TypeRef,
) -> Result<SlotId, SchemaError> {
    if build.models[model.0].slots.contains_key(&name) {
        return Err(build.error(field, "duplicate storage slot"));
    }
    let id = SlotId(build.slots.len());
    build.models[model.0].slots.insert(name.clone(), id);
    if !ty.nullable() {
        build.models[model.0].required_slots.push(id);
    }
    if build.models[model.0].key == Some(field) {
        build.models[model.0].immutable_slots.insert(id);
    }
    build.slots.push(Slot {
        model,
        name,
        ty,
        field,
        constraints: build.graph.fields()[field.0].constraints.clone(),
    });
    Ok(id)
}
