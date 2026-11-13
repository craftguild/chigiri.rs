//! Reserve row, identity, reference-key, and foreign-key indexes before consuming logical capacity.

use std::collections::BTreeMap;

use chigiri::core::Value;

use super::{StoreError, Usage};
use crate::ir::{MockIr, ModelId, RelationStorage, SlotId};
use crate::value::ValueBudget;

pub(super) fn reserve(
    ir: &MockIr,
    model: ModelId,
    values: &BTreeMap<SlotId, &Value>,
    budget: &mut ValueBudget,
) -> Result<Usage, StoreError> {
    let mut usage = Usage::default();
    entry(&mut usage, 16, budget)?;
    if let Some(slot) = super::records::key(ir, model)
        && let Some(value) = values.get(&slot).filter(|value| !value.is_null())
    {
        entry(&mut usage, scalar_bytes(value) + 8, budget)?;
    }
    for key in ir.keys() {
        budget.work.charge(1)?;
        if key.model == model {
            let bytes = key
                .slots
                .iter()
                .map(|slot| values.get(slot).map_or(1, |value| scalar_bytes(value)) + 1)
                .sum::<usize>();
            entry(&mut usage, bytes + 8, budget)?;
        }
    }
    for relation in ir.relations() {
        budget.work.charge(1)?;
        if relation.owner != model {
            continue;
        }
        let RelationStorage::ForeignKey { slots, .. } = &relation.storage else {
            continue;
        };
        // Charge each component conservatively even though the implementation shares tuples; null creates no index entry.
        for slot in slots {
            match values.get(slot).copied() {
                None | Some(Value::Null) => {}
                Some(Value::List(items)) => {
                    for item in items.iter().filter(|value| !value.is_null()) {
                        entry(&mut usage, scalar_bytes(item) + 17, budget)?;
                    }
                }
                Some(value) => entry(&mut usage, scalar_bytes(value) + 17, budget)?,
            }
        }
    }
    Ok(usage)
}

fn scalar_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len(),
        _ => 8,
    }
}

fn entry(usage: &mut Usage, bytes: usize, budget: &mut ValueBudget) -> Result<(), StoreError> {
    budget.container(1)?;
    budget.scratch(bytes)?;
    usage.nodes += 1;
    usage.bytes += bytes;
    Ok(())
}
