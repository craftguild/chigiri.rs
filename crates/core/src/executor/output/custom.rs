//! Iteratively validate custom-scalar JSON values under resource limits.

use crate::value::drop_iterative;
use crate::{ExecutionLimits, Value};

pub(crate) struct ValidatedCustomScalar {
    value: Value,
    additional_nodes: usize,
}

impl ValidatedCustomScalar {
    pub(crate) fn into_parts(self) -> (Value, usize) {
        (self.value, self.additional_nodes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustomScalarOutputError {
    ResourceLimit,
    InvalidNumeric,
}

pub(crate) fn validate_custom_scalar(
    value: Value,
    limits: &ExecutionLimits,
) -> Result<ValidatedCustomScalar, CustomScalarOutputError> {
    let result = match &value {
        Value::Null | Value::Boolean(_) | Value::Int(_) | Value::String(_) => Ok(0),
        Value::BigInt(raw) => valid_big_int(raw)
            .then_some(0)
            .ok_or(CustomScalarOutputError::InvalidNumeric),
        Value::Float(number) => number
            .is_finite()
            .then_some(0)
            .ok_or(CustomScalarOutputError::InvalidNumeric),
        Value::List(_) | Value::Object(_) => inspect_container(&value, limits),
    };
    match result {
        Ok(additional_nodes) => Ok(ValidatedCustomScalar {
            value,
            additional_nodes,
        }),
        Err(error) => {
            drop_iterative(value);
            Err(error)
        }
    }
}

fn inspect_container(
    root: &Value,
    limits: &ExecutionLimits,
) -> Result<usize, CustomScalarOutputError> {
    let mut additional_nodes = 0usize;
    let mut work = vec![(root, 0usize)];
    while let Some((value, depth)) = work.pop() {
        if depth > limits.max_custom_scalar_output_depth {
            return Err(CustomScalarOutputError::ResourceLimit);
        }
        match value {
            Value::List(items) => {
                if items.len() > limits.max_list_items_per_field {
                    return Err(CustomScalarOutputError::ResourceLimit);
                }
                push_children(
                    &mut work,
                    items.iter(),
                    depth,
                    &mut additional_nodes,
                    limits.max_output_nodes,
                )?;
            }
            Value::Object(object) => push_children(
                &mut work,
                object.iter().map(|(_, value)| value),
                depth,
                &mut additional_nodes,
                limits.max_output_nodes,
            )?,
            Value::BigInt(raw) if !valid_big_int(raw) => {
                return Err(CustomScalarOutputError::InvalidNumeric);
            }
            Value::Float(number) if !number.is_finite() => {
                return Err(CustomScalarOutputError::InvalidNumeric);
            }
            Value::Null
            | Value::Boolean(_)
            | Value::Int(_)
            | Value::BigInt(_)
            | Value::Float(_)
            | Value::String(_) => {}
        }
    }
    Ok(additional_nodes)
}

fn push_children<'a>(
    work: &mut Vec<(&'a Value, usize)>,
    children: impl Iterator<Item = &'a Value>,
    depth: usize,
    additional_nodes: &mut usize,
    maximum_nodes: usize,
) -> Result<(), CustomScalarOutputError> {
    for child in children {
        *additional_nodes = additional_nodes
            .checked_add(1)
            .filter(|count| *count <= maximum_nodes)
            .ok_or(CustomScalarOutputError::ResourceLimit)?;
        let child_depth = depth
            .checked_add(1)
            .ok_or(CustomScalarOutputError::ResourceLimit)?;
        work.push((child, child_depth));
    }
    Ok(())
}

fn valid_big_int(raw: &str) -> bool {
    crate::numeric::valid_big_int_lexeme(raw)
}
