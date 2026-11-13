//! Validate state types while copying; merge only fields explicitly present in a patch.

use chigiri::core::{Object, Value};

use super::{StateFields, StateType};
use crate::value::{MAX_STRING_BYTES, ValueBudget, ValueError, exact_number};

const SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub(super) fn object(
    fields: &StateFields,
    value: &Value,
    previous: Option<&Object>,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<Object, ValueError> {
    budget.container(depth)?;
    let Value::Object(object) = value else {
        return Err(ValueError("state object required"));
    };
    for (name, _) in object {
        budget.key(name)?;
        if !fields.contains_key(name) {
            return Err(ValueError("unknown state field"));
        }
    }
    let mut result = Object::new();
    for (name, ty) in fields {
        budget.key(name)?;
        let value = object
            .get(name)
            .or_else(|| previous.and_then(|previous| previous.get(name)));
        let value = match value {
            Some(value) => typed(ty, value, budget, depth + 1)?,
            None if ty.nullable() => {
                budget.node(&Value::Null, depth + 1)?;
                Value::Null
            }
            None => return Err(ValueError("required state field missing")),
        };
        result.insert(name.clone(), value);
    }
    Ok(result)
}

fn typed(
    ty: &StateType,
    value: &Value,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<Value, ValueError> {
    // nullable does not add value container, but the depth of the declaration is separately checked.
    if let StateType::Nullable { of } = ty {
        if value.is_null() {
            budget.node(value, depth)?;
            return Ok(Value::Null);
        }
        return typed(of, value, budget, depth);
    }
    if let StateType::Object { fields } = ty {
        return object(fields, value, None, budget, depth).map(Value::Object);
    }
    budget.node(value, depth)?;
    Ok(match (ty, value) {
        (StateType::String, Value::String(text)) if text.len() <= MAX_STRING_BYTES => {
            Value::String(text.clone())
        }
        (StateType::Boolean, Value::Boolean(value)) => Value::Boolean(*value),
        (StateType::Integer, Value::Int(_) | Value::Float(_)) => {
            let number = exact_number(value)?;
            if number.fract() != 0.0 || number.abs() > SAFE_INTEGER {
                return Err(ValueError("safe state integer required"));
            }
            Value::Int(number as i128)
        }
        (StateType::Number, Value::Int(_) | Value::BigInt(_) | Value::Float(_)) => {
            exact_number(value)?;
            value.clone()
        }
        (StateType::List { of }, Value::List(values)) => {
            budget.items(values.len())?;
            let mut result = Vec::new();
            for value in values {
                result.push(typed(of, value, budget, depth + 1)?);
            }
            Value::List(result)
        }
        _ => return Err(ValueError("state value does not match declared type")),
    })
}
