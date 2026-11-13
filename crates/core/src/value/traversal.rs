//! Shared iterative depth checks and destruction for discarded [`Value`] trees.

use super::Value;

pub(crate) fn within_depth_limit(value: &Value, maximum: usize) -> bool {
    let mut work = vec![(value, 0_usize)];
    while let Some((current, depth)) = work.pop() {
        if depth > maximum {
            return false;
        }
        match current {
            Value::List(items) => {
                if items.is_empty() {
                    continue;
                }
                let Some(child_depth) = depth.checked_add(1) else {
                    return false;
                };
                work.extend(items.iter().map(|item| (item, child_depth)));
            }
            Value::Object(object) => {
                if object.is_empty() {
                    continue;
                }
                let Some(child_depth) = depth.checked_add(1) else {
                    return false;
                };
                work.extend(object.iter().map(|(_, item)| (item, child_depth)));
            }
            Value::Null
            | Value::Boolean(_)
            | Value::Int(_)
            | Value::BigInt(_)
            | Value::Float(_)
            | Value::String(_) => {}
        }
    }
    true
}

/// Iteratively decomposes even over-limit values so recursive `Drop` does not
/// consume the process stack.
pub(crate) fn drop_iterative(value: Value) {
    let mut work = vec![value];
    while let Some(current) = work.pop() {
        match current {
            Value::List(items) => work.extend(items),
            Value::Object(object) => work.extend(object.into_iter().map(|(_, value)| value)),
            Value::Null
            | Value::Boolean(_)
            | Value::Int(_)
            | Value::BigInt(_)
            | Value::Float(_)
            | Value::String(_) => {}
        }
    }
}
