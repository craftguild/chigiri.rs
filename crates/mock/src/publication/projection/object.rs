//! Preserve captured object origins while resolving all IDs in the fresh schema owner.

use std::collections::BTreeMap;

use super::value;
use crate::ir::{FieldPlan, TypeId};
use crate::output::{OutputValue, captured_fields, captured_record};
use crate::publication::capture::{CapturedObject, CapturedOrigin};
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError};

pub(super) fn project(
    captured: &CapturedObject,
    snapshot: &Snapshot,
    expected: TypeId,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<OutputValue, ValueError> {
    budget.container(depth)?;
    let ty = snapshot
        .ir()
        .graph()
        .type_id(&captured.type_name)
        .ok_or(ValueError("unknown captured object type"))?;
    let expected_node = &snapshot.ir().graph().types()[expected.0];
    if expected != ty && expected_node.possible_types.binary_search(&ty).is_err() {
        return Err(ValueError("captured object is not a possible output type"));
    }
    match &captured.origin {
        CapturedOrigin::Dto => fields(captured, snapshot, ty, false, budget, depth),
        CapturedOrigin::Stored => fields(captured, snapshot, ty, true, budget, depth),
        CapturedOrigin::Record {
            model,
            public_identity,
            ..
        } => record(
            captured,
            snapshot,
            ty,
            model,
            public_identity.as_deref(),
            budget,
            depth,
        ),
    }
}

fn fields(
    captured: &CapturedObject,
    snapshot: &Snapshot,
    ty: TypeId,
    stored: bool,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<OutputValue, ValueError> {
    budget.items(captured.fields.len())?;
    let ir = snapshot.ir();
    let node = &ir.graph().types()[ty.0];
    let mut fields = BTreeMap::new();
    for captured in &captured.fields {
        budget.key_bytes(captured.name.len())?;
        let field = *node
            .fields
            .get(captured.name.as_ref())
            .ok_or(ValueError("unknown captured output field"))?;
        if !matches!(
            ir.field_plan(field),
            Some(FieldPlan::Value | FieldPlan::Stored(_))
        ) || (!stored && ir.is_private_field(field))
            || fields.contains_key(&field)
        {
            return Err(ValueError("invalid captured output field"));
        }
        fields.insert(
            field,
            value::typed(
                &captured.value,
                snapshot,
                &ir.graph().fields()[field.0].ty,
                budget,
                depth + 1,
            )?,
        );
    }
    if ir
        .required_value_fields(ty)
        .iter()
        .any(|field| (stored || !ir.is_private_field(*field)) && !fields.contains_key(field))
    {
        return Err(ValueError("required captured output field missing"));
    }
    captured_fields(snapshot.clone(), ty, fields, stored)
}

fn record(
    captured: &CapturedObject,
    snapshot: &Snapshot,
    ty: TypeId,
    model_name: &str,
    identity: Option<&str>,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<OutputValue, ValueError> {
    budget.items(captured.fields.len())?;
    let ir = snapshot.ir();
    let model = ir
        .model_for(ty)
        .filter(|model| ir.graph().types()[ir.models()[model.0].ty.0].name.as_ref() == model_name)
        .ok_or(ValueError("invalid captured record model"))?;
    let definition = &ir.models()[model.0];
    let mut slots = BTreeMap::new();
    for field in &captured.fields {
        budget.key_bytes(field.name.len())?;
        let slot = *definition
            .slots
            .get(field.name.as_ref())
            .ok_or(ValueError("unknown captured record slot"))?;
        if slots.contains_key(&slot) {
            return Err(ValueError("duplicate captured record slot"));
        }
        slots.insert(slot, value::scalar(&field.value, budget, depth + 1)?);
    }
    let identity = identity
        .map(|identity| {
            budget.string_node(identity.len(), depth + 1)?;
            Ok::<_, ValueError>(identity.to_owned().into_boxed_str())
        })
        .transpose()?;
    captured_record(snapshot.clone(), ty, model, slots, identity)
}
