//! Apply built-in and declared custom-scalar constraints to the same value.

use chigiri::core::Value;

use super::{MAX_STRING_BYTES, ValueBudget, ValueError, constraints, exact_number};
use crate::ir::{Representation, ScalarSpec};

pub(super) fn builtin(name: &str, value: &Value, identity: bool) -> Result<(), ValueError> {
    let valid = match (name, value) {
        ("Int", Value::Int(value)) => i32::try_from(*value).is_ok(),
        ("Float", _) => return exact_number(value).map(|_| ()),
        ("Boolean", Value::Boolean(_)) => true,
        ("ID", Value::String(text)) => !text.is_empty() && text.len() <= crate::limits::ID_BYTES,
        ("String", Value::String(text)) if identity => {
            !text.is_empty() && text.len() <= crate::limits::ID_BYTES
        }
        ("String", Value::String(text)) => text.len() <= MAX_STRING_BYTES,
        _ => false,
    };
    valid
        .then_some(())
        .ok_or(ValueError("invalid scalar value"))
}

pub(super) fn custom(
    spec: &ScalarSpec,
    value: &Value,
    depth: usize,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    let valid = match (spec.representation, value) {
        (Representation::String, Value::String(text)) => text.len() <= MAX_STRING_BYTES,
        (Representation::Boolean, Value::Boolean(_)) => true,
        (Representation::Integer, _) => exact_number(value)?.fract() == 0.0,
        (Representation::Number, _) => {
            exact_number(value)?;
            true
        }
        (Representation::Object, Value::Object(_))
        | (Representation::Array, Value::List(_))
        | (Representation::Json, _) => true,
        _ => false,
    };
    if !valid {
        return Err(ValueError("custom scalar representation"));
    }
    constraints(&spec.constraints, value, budget)?;
    json_children(value, depth, budget)
}

fn json_children(value: &Value, depth: usize, budget: &mut ValueBudget) -> Result<(), ValueError> {
    match value {
        Value::List(items) => {
            budget.items(items.len())?;
            for value in items {
                budget.node(value, depth + 1)?;
                json_children(value, depth + 1, budget)?;
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key.len() > MAX_STRING_BYTES {
                    return Err(ValueError("JSON key length"));
                }
                budget.key(key)?;
                budget.node(value, depth + 1)?;
                json_children(value, depth + 1, budget)?;
            }
        }
        Value::String(text) if text.len() > MAX_STRING_BYTES => {
            return Err(ValueError("string length"));
        }
        Value::Int(_) | Value::BigInt(_) | Value::Float(_) => {
            exact_number(value)?;
        }
        _ => {}
    }
    Ok(())
}
