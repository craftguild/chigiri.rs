//! Validate patches before selection so an invalid patch cannot succeed by matching no records.

use chigiri::core::{Object, Value};

use super::{StoreError, relations, work::Work};
use crate::ir::{FieldNode, MockIr, ModelId, RelationStorage, SlotId};
use crate::value::{ValueBudget, ValueError, ValueValidator};

pub(super) fn validate(
    ir: &MockIr,
    model: ModelId,
    patch: &Object,
    budget: &mut ValueBudget,
) -> Result<(), StoreError> {
    let definition = ir
        .models()
        .get(model.0)
        .ok_or(ValueError("unknown model"))?;
    budget.container(1)?;
    // Validate field ownership, key immutability, types, constraints, and nullability.
    for (name, value) in patch {
        budget.key(name)?;
        let slot = definition
            .slots
            .get(name)
            .copied()
            .ok_or(ValueError("unknown or computed storage field"))?;
        if definition.immutable_slots.contains(&slot) {
            return Err(ValueError("model key is immutable"));
        }
        ValueValidator::new(ir, budget).slot_at_depth(slot, value, 2)?;
    }
    for relation in ir.relations() {
        budget.work.charge(1)?;
        if relation.owner == model
            && let RelationStorage::ForeignKey { slots, .. } = &relation.storage
        {
            foreign_key(
                ir,
                &ir.graph().fields()[relation.field.0],
                slots,
                patch,
                &mut budget.work,
            )?;
        }
    }
    Ok(())
}

fn foreign_key(
    ir: &MockIr,
    field: &FieldNode,
    slots: &[SlotId],
    patch: &Object,
    work: &mut Work,
) -> Result<(), StoreError> {
    work.charge(slots.len())?;
    let mut first: Option<&Value> = None;
    for slot in slots {
        let Some(value) = patch.get(ir.slots()[slot.0].name.as_ref()) else {
            // Unspecified components retain their stored values. Reject only contradictions
            // that can already be established among the supplied components.
            continue;
        };
        if let Some(previous) = first {
            if previous.is_null() != value.is_null() {
                return Err(ValueError("partial null composite reference"));
            }
            if let (Value::List(left), Value::List(right)) = (previous, value) {
                if left.len() != right.len() {
                    return Err(ValueError("composite FK array lengths differ"));
                }
                work.charge(left.len())?;
                if left
                    .iter()
                    .zip(right)
                    .any(|(left, right)| left.is_null() != right.is_null())
                {
                    return Err(ValueError("partial null composite reference"));
                }
            }
        } else {
            if let Value::List(values) = value {
                relations::owner_cardinality(field, values.len())?;
            }
            first = Some(value);
        }
    }
    Ok(())
}
