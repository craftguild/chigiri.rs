//! Separate outer list constraints from scalar-leaf constraints and apply one policy to every element.

use chigiri::core::Value;

use super::{ValueBudget, ValueError, decimal, exact_number, formats};
use crate::ir::Constraints;

pub(crate) fn list(
    bounds: &Constraints,
    length: usize,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    budget.work.charge(1)?;
    within(length, bounds.min_items, bounds.max_items)
        .then_some(())
        .ok_or(ValueError("list constraint"))
}

pub(crate) fn constraints(
    bounds: &Constraints,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    budget.work.charge(0)?;
    if value.is_null() {
        return Ok(());
    }
    if bounds.min_items.is_some() || bounds.max_items.is_some() {
        let Value::List(items) = value else {
            return Err(ValueError("list constraint"));
        };
        list(bounds, items.len(), budget)?;
    }
    leaf(bounds, value, budget)
}

pub(crate) fn leaf(
    bounds: &Constraints,
    value: &Value,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    budget.work.charge(1)?;
    if value.is_null() {
        return Ok(());
    }
    if bounds.min.is_some()
        || bounds.max.is_some()
        || bounds.exclusive_min.is_some()
        || bounds.exclusive_max.is_some()
        || bounds.multiple_of.is_some()
    {
        let number = exact_number(value)?;
        if !within(number, bounds.min, bounds.max)
            || bounds.exclusive_min.is_some_and(|min| number <= min)
            || bounds.exclusive_max.is_some_and(|max| number >= max)
            || bounds
                .multiple_of
                .map(|divisor| decimal::multiple(number, divisor, &mut budget.work))
                .transpose()?
                .is_some_and(|multiple| !multiple)
        {
            return Err(ValueError("number constraint"));
        }
    }
    if let Value::String(text) = value {
        if text.len() > super::MAX_STRING_BYTES {
            return Err(ValueError("string length"));
        }
        // Count characters and evaluate substring constraints only after enforcing the 4 KiB cap.
        budget.work.charge(1 + 6 * text.len())?;
        if !within(text.len(), bounds.min_bytes, bounds.max_bytes)
            || !within(text.chars().count(), bounds.min_length, bounds.max_length)
            || bounds
                .starts_with
                .as_ref()
                .is_some_and(|prefix| !text.starts_with(prefix.as_ref()))
            || bounds
                .ends_with
                .as_ref()
                .is_some_and(|suffix| !text.ends_with(suffix.as_ref()))
            || bounds
                .contains
                .as_ref()
                .is_some_and(|part| !text.contains(part.as_ref()))
            || bounds
                .not_contains
                .as_ref()
                .is_some_and(|part| text.contains(part.as_ref()))
        {
            return Err(ValueError("string constraint"));
        }
        if let Some(format) = bounds.format {
            formats::validate(format, text, budget)?;
        }
        if let Some(pattern) = &bounds.pattern {
            budget.work.reserve_match(text.len())?;
            if !pattern.is_match(text, || budget.work.charge(0))? {
                return Err(ValueError("string pattern constraint"));
            }
        }
    }
    Ok(())
}

fn within<T: PartialOrd + Copy>(value: T, min: Option<T>, max: Option<T>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}
