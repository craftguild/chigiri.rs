//! Finds model files pulled into a scoped seed check by explicit references.

use std::collections::BTreeSet;

use chigiri::core::schema::TypeKind;
use chigiri::core::{Object, Value};

use super::StoreError;
use crate::api::model_values::TYPENAME;
use crate::ir::{MockIr, ModelId, TypeRef, Wrapper};
use crate::value::{MAX_DEPTH, ValueError};

pub(super) fn dependencies(
    ir: &MockIr,
    model: ModelId,
    row: &Object,
) -> Result<BTreeSet<ModelId>, StoreError> {
    let mut found = BTreeSet::new();
    for (name, value) in row {
        let Some(slot) = ir.models()[model.0].slots.get(name) else {
            continue;
        };
        visit(ir, &ir.slots()[slot.0].ty, 0, value, 1, &mut found)?;
    }
    Ok(found)
}

fn visit(
    ir: &MockIr,
    ty: &TypeRef,
    mut wrapper: usize,
    value: &Value,
    depth: usize,
    found: &mut BTreeSet<ModelId>,
) -> Result<(), StoreError> {
    if depth > MAX_DEPTH {
        return Err(ValueError("seed reference depth limit"));
    }
    if ty.wrappers.get(wrapper) == Some(&Wrapper::NonNull) {
        wrapper += 1;
    } else if value.is_null() {
        return Ok(());
    }
    if ty.wrappers.get(wrapper) == Some(&Wrapper::List) {
        if let Value::List(items) = value {
            for item in items {
                visit(ir, ty, wrapper + 1, item, depth + 1, found)?;
            }
        }
        return Ok(());
    }
    let kind = ir.graph().types()[ty.named.0].kind;
    if !matches!(
        kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    ) {
        return Ok(());
    }
    let Value::Object(object) = value else {
        return Ok(());
    };
    if let Some(Value::Object(reference)) = object.get("$ref") {
        if let Some(model) = reference
            .get("model")
            .and_then(Value::as_str)
            .and_then(|name| ir.graph().type_id(name))
            .and_then(|ty| ir.model_for(ty))
        {
            found.insert(model);
        }
        return Ok(());
    }
    let concrete = if kind == TypeKind::Object {
        ty.named
    } else if let Some(concrete) = object
        .get(TYPENAME)
        .and_then(Value::as_str)
        .and_then(|name| ir.graph().type_id(name))
    {
        concrete
    } else {
        return Ok(());
    };
    for (name, value) in object {
        let Some(field) = ir.graph().types()[concrete.0].fields.get(name) else {
            continue;
        };
        visit(
            ir,
            &ir.graph().fields()[field.0].ty,
            0,
            value,
            depth + 1,
            found,
        )?;
    }
    Ok(())
}
