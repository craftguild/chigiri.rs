//! Convert custom-scalar literals into present, coerced core values.

use std::collections::BTreeSet;

use crate::language::ast::{Value as AstValue, ValueNode};
use crate::{Object, Value};

use super::engine::{Engine, Source};
use super::{
    CoercedInput, INPUT_DUPLICATE_FIELD, INPUT_NESTING_LIMIT, INPUT_SCALAR_VALUE,
    INPUT_VARIABLE_UNAVAILABLE,
};

pub(super) fn convert(engine: &mut Engine<'_>, source: Source<'_>, depth: usize) -> Option<Value> {
    match source {
        Source::Raw(value) => {
            if !engine.reserve_value(&value, None) {
                crate::value::drop_iterative(value);
                return None;
            }
            Some(value)
        }
        Source::Literal(node) => literal(engine, node, depth)?.into_value(),
    }
}

fn literal(engine: &mut Engine<'_>, node: &ValueNode, depth: usize) -> Option<CoercedInput> {
    if depth > engine.maximum_depth {
        engine.report(
            INPUT_NESTING_LIMIT,
            "Input value nesting limit exceeded",
            Some(node.span),
        );
        return None;
    }
    if !engine.reserve(
        1,
        super::budget::source_bytes(&Source::Literal(node)),
        Some(node.span),
    ) {
        return None;
    }
    match &node.value {
        AstValue::Variable(name) => match engine
            .variables
            .and_then(|variables| variables.get(&name.value))
        {
            Some(CoercedInput::Value(value)) => {
                if !engine.reserve_value(value, Some(node.span)) {
                    return None;
                }
                Some(CoercedInput::Value(value.clone()))
            }
            Some(value) => Some(value.clone()),
            None => {
                engine.report(
                    INPUT_VARIABLE_UNAVAILABLE,
                    format!("Variable `${}` has not been coerced", name.value),
                    Some(name.span),
                );
                None
            }
        },
        AstValue::Int(raw) => crate::numeric::value_from_integer_lexeme(raw)
            .map(CoercedInput::Value)
            .or_else(|| invalid_number(engine, node)),
        AstValue::Float(raw) => {
            match crate::numeric::finite_float_from_lexeme(raw).filter(|value| {
                // In custom scalar/JSON, we compare the integers before rounding.
                engine.budget.is_none() || crate::numeric::integer_float_matches_lexeme(raw, *value)
            }) {
                Some(value) => Some(CoercedInput::Value(Value::Float(value))),
                None => invalid_number(engine, node),
            }
        }
        AstValue::String(value) => Some(CoercedInput::Value(Value::String(value.value.clone()))),
        AstValue::Boolean(value) => Some(CoercedInput::Value(Value::Boolean(*value))),
        AstValue::Null => Some(CoercedInput::Null),
        AstValue::Enum(name) => Some(CoercedInput::Value(Value::String(name.value.clone()))),
        AstValue::List(items) => list(engine, items, depth.saturating_add(1)),
        AstValue::Object(fields) => {
            let mut seen = BTreeSet::new();
            let mut output = Object::with_capacity(fields.len());
            for field in fields {
                if !engine.reserve(0, field.name.value.len(), Some(field.name.span)) {
                    return None;
                }
                if !seen.insert(field.name.value.as_str()) {
                    engine.report(
                        INPUT_DUPLICATE_FIELD,
                        format!(
                            "Input field `{}` is provided more than once",
                            field.name.value
                        ),
                        Some(field.name.span),
                    );
                    continue;
                }
                let value = literal(engine, &field.value, depth.saturating_add(1))
                    .and_then(CoercedInput::into_value);
                if let Some(value) = value {
                    output.insert(field.name.value.clone(), value);
                }
                if !engine.can_continue() {
                    return None;
                }
            }
            Some(CoercedInput::Value(Value::Object(output)))
        }
    }
}

fn list(engine: &mut Engine<'_>, items: &[ValueNode], depth: usize) -> Option<CoercedInput> {
    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        engine.push_path(format!("[{index}]"));
        match literal(engine, item, depth) {
            Some(CoercedInput::Value(value)) => output.push(value),
            Some(CoercedInput::Null | CoercedInput::Undefined) => output.push(Value::Null),
            None => {}
        }
        engine.pop_path();
        if !engine.can_continue() {
            return None;
        }
    }
    Some(CoercedInput::Value(Value::List(output)))
}

fn invalid_number(engine: &mut Engine<'_>, node: &ValueNode) -> Option<CoercedInput> {
    engine.report(
        INPUT_SCALAR_VALUE,
        "Numeric literal cannot be represented as a core value",
        Some(node.span),
    );
    None
}
