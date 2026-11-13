//! Index schema-defined key tuples while preserving component boundaries and scalar type identity.

use std::sync::Arc;

use chigiri::core::Value;

use super::{Record, State, StoreError, work::Work};
use crate::ir::{KeyId, MockIr, SlotId, TypeId};
use crate::output::CapturedRecord;
use crate::value::{ValueBudget, ValueError};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Scalar {
    String(Arc<str>),
    Int(i32),
    Boolean(bool),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct KeyValue(Vec<(TypeId, Scalar)>);

impl KeyValue {
    pub(super) fn cost(&self) -> usize {
        self.0
            .iter()
            .map(|(_, value)| match value {
                Scalar::String(text) => 1 + text.len(),
                _ => 9,
            })
            .sum()
    }

    pub(super) fn value(&self, position: usize) -> Option<Value> {
        self.0
            .get(position)
            .map(|(_, value)| materialize_scalar(value))
    }

    pub(super) fn reserve_component(
        &self,
        position: usize,
        budget: &mut ValueBudget,
        depth: usize,
    ) -> Result<(), ValueError> {
        match self.0.get(position).map(|(_, value)| value) {
            Some(Scalar::String(text)) => budget.string_node(text.len(), depth),
            Some(Scalar::Int(_)) => budget.node(&Value::Int(0.into()), depth),
            Some(Scalar::Boolean(_)) => budget.node(&Value::Boolean(false), depth),
            None => budget.node(&Value::Null, depth),
        }
    }
}

fn materialize_scalar(value: &Scalar) -> Value {
    match value {
        Scalar::String(text) => Value::from(text.as_ref()),
        Scalar::Int(value) => Value::Int((*value).into()),
        Scalar::Boolean(value) => Value::Boolean(*value),
    }
}

pub(super) fn tuple<'a>(
    ir: &MockIr,
    key: KeyId,
    values: impl Iterator<Item = Option<&'a Value>>,
    work: &mut Work,
) -> Result<Option<KeyValue>, StoreError> {
    let spec = &ir.keys()[key.0];
    work.charge(spec.slots.len())?;
    let mut parts = Vec::with_capacity(spec.slots.len());
    let mut nulls = 0;
    for (slot, value) in spec.slots.iter().zip(values) {
        let named = ir.slots()[slot.0].ty.named;
        let scalar = match value {
            None | Some(Value::Null) => {
                nulls += 1;
                continue;
            }
            Some(Value::String(text)) => {
                if text.is_empty() || text.len() > crate::limits::ID_BYTES {
                    return Err(ValueError("invalid reference key string"));
                }
                work.charge(1 + text.len())?;
                Scalar::String(Arc::from(text.as_str()))
            }
            Some(Value::Int(value)) => Scalar::Int(
                i32::try_from(*value).map_err(|_| ValueError("reference Int outside i32"))?,
            ),
            Some(Value::Boolean(value)) => Scalar::Boolean(*value),
            _ => return Err(ValueError("invalid reference key scalar")),
        };
        parts.push((named, scalar));
    }
    if parts.len() + nulls != spec.slots.len() {
        return Err(ValueError("reference key component count"));
    }
    if nulls == spec.slots.len() {
        return Ok(None);
    }
    if nulls != 0 {
        return Err(ValueError("partial null composite reference"));
    }
    Ok(Some(KeyValue(parts)))
}

pub(super) fn record(
    ir: &MockIr,
    key: KeyId,
    record: &Record,
    work: &mut Work,
) -> Result<KeyValue, StoreError> {
    tuple(
        ir,
        key,
        ir.keys()[key.0].slots.iter().map(|slot| record.get(*slot)),
        work,
    )?
    .ok_or(ValueError("reference key cannot be null"))
}

pub(super) fn captured_record(
    ir: &MockIr,
    key: KeyId,
    record: &CapturedRecord,
    work: &mut Work,
) -> Result<KeyValue, StoreError> {
    tuple(
        ir,
        key,
        ir.keys()[key.0].slots.iter().map(|slot| record.get(*slot)),
        work,
    )?
    .ok_or(ValueError("reference key cannot be null"))
}

pub(super) fn replace(
    ir: &MockIr,
    state: &mut State,
    old: Option<&Record>,
    new: Option<&Record>,
    work: &mut Work,
) -> Result<(), StoreError> {
    for (row, remove) in old
        .into_iter()
        .map(|row| (row, true))
        .chain(new.map(|row| (row, false)))
    {
        for (index, spec) in ir.keys().iter().enumerate() {
            work.charge(1)?;
            if spec.model != row.model {
                continue;
            }
            let value = record(ir, KeyId(index), row, work)?;
            // Reserve the index copy required by copy-on-write before making it mutable.
            if Arc::strong_count(&state.keys[index]) > 1 {
                work.charge(
                    state.keys[index].len() * (1 + spec.slots.len() * crate::limits::ID_BYTES),
                )?;
            }
            let entries = Arc::make_mut(&mut state.keys[index]);
            if remove {
                entries.remove(&value);
            } else if entries.insert(value, row.order).is_some() {
                return Err(ValueError("duplicate reference key tuple"));
            }
        }
    }
    Ok(())
}

pub(super) fn references(
    ir: &MockIr,
    key: KeyId,
    slots: &[SlotId],
    row: &Record,
    work: &mut Work,
) -> Result<Option<Vec<Option<KeyValue>>>, StoreError> {
    references_with(ir, key, slots, |slot| row.get(slot), work)
}

pub(super) fn captured_references(
    ir: &MockIr,
    key: KeyId,
    slots: &[SlotId],
    row: &CapturedRecord,
    work: &mut Work,
) -> Result<Option<Vec<Option<KeyValue>>>, StoreError> {
    references_with(ir, key, slots, |slot| row.get(slot), work)
}

fn references_with<'a>(
    ir: &MockIr,
    key: KeyId,
    slots: &[SlotId],
    value: impl Fn(SlotId) -> Option<&'a Value>,
    work: &mut Work,
) -> Result<Option<Vec<Option<KeyValue>>>, StoreError> {
    let first = slots.first().ok_or(ValueError("empty FK components"))?;
    work.charge(slots.len())?;
    let list = ir.slots()[first.0].ty.list_depth() != 0;
    if !list {
        return tuple(ir, key, slots.iter().map(|slot| value(*slot)), work)
            .map(|key| key.map(|key| vec![Some(key)]));
    }
    let mut length = None;
    let mut missing = 0;
    for slot in slots {
        match value(*slot) {
            None | Some(Value::Null) => missing += 1,
            Some(Value::List(values)) => {
                if length.is_some_and(|length| length != values.len()) {
                    return Err(ValueError("composite FK array lengths differ"));
                }
                length = Some(values.len());
            }
            _ => return Err(ValueError("FK array required")),
        }
    }
    if missing == slots.len() {
        return Ok(None);
    }
    if missing != 0 {
        return Err(ValueError("partial null composite reference"));
    }
    let length = length.unwrap_or(0);
    work.charge(length.saturating_mul(slots.len()))?;
    (0..length)
        .map(|index| {
            tuple(
                ir,
                key,
                slots.iter().map(|slot| match value(*slot) {
                    Some(Value::List(values)) => values.get(index),
                    _ => None,
                }),
                work,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
