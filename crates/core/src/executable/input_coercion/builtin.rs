//! Canonicalise the built-in scalar input to core `Value`.

use crate::Value;
use crate::language::ast::Value as AstValue;

use super::engine::Source;

pub(super) fn int(source: Source<'_>) -> Option<Value> {
    let value = match source {
        Source::Raw(Value::Int(value)) => i32::try_from(value).ok()?,
        Source::Raw(Value::BigInt(_)) => return None,
        Source::Literal(node) => match &node.value {
            AstValue::Int(raw) => raw.parse::<i32>().ok()?,
            _ => return None,
        },
        Source::Raw(_) => return None,
    };
    Some(Value::Int(i128::from(value)))
}

pub(super) fn float(source: Source<'_>) -> Option<Value> {
    let value = match source {
        Source::Raw(Value::Int(value)) => crate::numeric::exact_float_from_integer(value)?,
        Source::Raw(Value::BigInt(raw)) if crate::numeric::valid_big_int_lexeme(&raw) => {
            crate::numeric::exact_float_from_integer_lexeme(&raw)?
        }
        Source::Raw(Value::Float(value)) => value,
        Source::Literal(node) => match &node.value {
            AstValue::Int(raw) => crate::numeric::exact_float_from_integer_lexeme(raw)?,
            AstValue::Float(raw) => crate::numeric::finite_float_from_lexeme(raw)?,
            _ => return None,
        },
        Source::Raw(_) => return None,
    };
    value.is_finite().then_some(Value::Float(value))
}

pub(super) fn string(source: Source<'_>) -> Option<Value> {
    match source {
        Source::Raw(Value::String(value)) => Some(Value::String(value)),
        Source::Literal(node) => match &node.value {
            AstValue::String(value) => Some(Value::String(value.value.clone())),
            _ => None,
        },
        Source::Raw(_) => None,
    }
}

pub(super) fn boolean(source: Source<'_>) -> Option<Value> {
    match source {
        Source::Raw(Value::Boolean(value)) => Some(Value::Boolean(value)),
        Source::Literal(node) => match node.value {
            AstValue::Boolean(value) => Some(Value::Boolean(value)),
            _ => None,
        },
        Source::Raw(_) => None,
    }
}

pub(super) fn id(source: Source<'_>) -> Option<Value> {
    match source {
        Source::Raw(Value::String(value)) => Some(Value::String(value)),
        Source::Raw(Value::Int(value)) => Some(Value::String(value.to_string())),
        Source::Raw(Value::BigInt(raw)) if crate::numeric::valid_big_int_lexeme(&raw) => {
            Some(Value::String(raw))
        }
        Source::Literal(node) => match &node.value {
            AstValue::String(value) => Some(Value::String(value.value.clone())),
            AstValue::Int(raw) => Some(Value::String(
                crate::numeric::canonical_id_from_integer_lexeme(raw)?,
            )),
            _ => None,
        },
        Source::Raw(_) => None,
    }
}
