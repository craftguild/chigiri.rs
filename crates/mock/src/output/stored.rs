//! Project host-read storage objects without granting independent record identity.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::Object;

use super::ObjectData;
use super::validate::{Mode, Validator};
use crate::codec::PathPart;
use crate::ir::{FieldPlan, TypeId};
use crate::value::ValueError;

pub(super) fn project(
    validator: &mut Validator<'_>,
    ty: TypeId,
    object: &Object,
    depth: usize,
) -> Result<ObjectData, ValueError> {
    let ir = validator.snapshot.ir();
    let node = &ir.graph().types()[ty.0];
    let mut fields = BTreeMap::new();
    for (name, value) in object {
        if name == "__typename" {
            continue;
        }
        validator.budget.key(name)?;
        let id = *node
            .fields
            .get(name)
            .ok_or(ValueError("unknown stored output field"))?;
        if !matches!(
            ir.field_plan(id),
            Some(FieldPlan::Value | FieldPlan::Stored(_))
        ) {
            validator.failure_member.get_or_insert(id);
            return Err(ValueError("computed stored output field supplied"));
        }
        validator.path.push(PathPart::Field(name.to_owned()));
        let field = &ir.graph().fields()[id.0];
        let validated = validator.typed(
            field.ty.named,
            &field.ty.wrappers,
            Some(&field.constraints),
            value,
            depth + 1,
            Mode {
                outer: true,
                inline: true,
                stored: true,
            },
        );
        validator.path.pop();
        if validated.is_err() {
            validator.failure_member.get_or_insert(id);
        }
        fields.insert(id, validated?);
    }
    for id in ir.required_value_fields(ty) {
        if !fields.contains_key(id) {
            validator.failure_member.get_or_insert(*id);
            return Err(ValueError("required stored output field missing"));
        }
    }
    Ok(ObjectData::Stored(Arc::new(fields)))
}
