//! External reference values are checked as single scalar or closed objects with references.

use chigiri::core::Value;

use super::{
    StoreError,
    keys::{self, KeyValue},
};
use crate::ir::{KeyId, MockIr};
use crate::value::{ValueBudget, ValueError, ValueValidator};

pub(super) fn parse(
    ir: &MockIr,
    key: KeyId,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<Option<KeyValue>, StoreError> {
    let spec = &ir.keys()[key.0];
    if value.is_null() {
        budget.node(value, 1)?;
        return Ok(None);
    }
    if spec.slots.len() == 1 {
        ValueValidator::new(ir, budget).slot(spec.slots[0], value)?;
        return keys::tuple(ir, key, std::iter::once(Some(value)), &mut budget.work);
    }
    let Value::Object(fields) = value else {
        return Err(ValueError("composite reference requires named object"));
    };
    budget.container(1)?;
    if fields.len() != spec.slots.len() {
        return Err(ValueError("composite reference component count"));
    }
    let mut values = Vec::with_capacity(spec.slots.len());
    for slot in &spec.slots {
        let name = &ir.slots()[slot.0].name;
        let value = fields
            .get(name)
            .ok_or(ValueError("composite reference component missing"))?;
        budget.key(name)?;
        if value.is_null() {
            budget.node(value, 2)?;
        } else {
            ValueValidator::new(ir, budget).slot_at_depth(*slot, value, 2)?;
        }
        values.push(Some(value));
    }
    keys::tuple(ir, key, values.into_iter(), &mut budget.work)
}
