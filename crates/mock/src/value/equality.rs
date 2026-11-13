//! Compare validated values while ignoring object insertion order and equivalent exact numeric representations.

use chigiri::core::Value;

use super::exact_number;

/// Compare values that have already passed node validation.
pub(crate) fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (
            Value::Int(_) | Value::BigInt(_) | Value::Float(_),
            Value::Int(_) | Value::BigInt(_) | Value::Float(_),
        ) => matches!((exact_number(left), exact_number(right)), (Ok(a), Ok(b)) if a == b),
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| equal(value, other)))
        }
        (Value::List(left), Value::List(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| equal(left, right))
        }
        _ => left == right,
    }
}
