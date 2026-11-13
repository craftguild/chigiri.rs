//! Accounts for structural nodes expanded from constant values into generated descriptors.

use chigiri_core::language::ast::Value;

use crate::error::CodegenError;

pub(super) fn value_units(root: &Value) -> Result<usize, CodegenError> {
    let mut units = 0usize;
    let mut pending = vec![root];
    while let Some(value) = pending.pop() {
        units = units.checked_add(1).ok_or_else(limit_error)?;
        match value {
            Value::List(values) => {
                pending.extend(values.iter().map(|value| &value.value));
            }
            Value::Object(fields) => {
                units = units.checked_add(fields.len()).ok_or_else(limit_error)?;
                pending.extend(fields.iter().map(|field| &field.value.value));
            }
            Value::Variable(_)
            | Value::Int(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Boolean(_)
            | Value::Null
            | Value::Enum(_) => {}
        }
    }
    Ok(units)
}

fn limit_error() -> CodegenError {
    CodegenError::new(
        "SF4010",
        "The number of structural nodes in const values exceeds the resource limit.",
    )
}

#[cfg(test)]
mod tests {
    use chigiri_core::language::ast::{Value, ValueNode};
    use chigiri_core::{SourceId, Span};

    use super::value_units;

    #[test]
    fn repeatedly_counting_list_and_object_fields() {
        let span = Span::new(SourceId::new(1), 0, 1);
        let list = Value::List(vec![
            ValueNode::new(Value::Int("0".to_owned()), span),
            ValueNode::new(Value::Null, span),
        ]);

        assert_eq!(value_units(&list).unwrap(), 3);
    }
}
