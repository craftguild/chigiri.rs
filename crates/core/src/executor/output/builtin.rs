//! Result coercion for enums and built-in scalars, independent of custom scalars.

use crate::Value;
use crate::schema::SemanticType;

pub(crate) fn is_builtin_scalar(type_name: &str) -> bool {
    matches!(type_name, "Int" | "Float" | "String" | "Boolean" | "ID")
}

pub(crate) fn coerce_enum(ty: &SemanticType, value: Value) -> Result<Value, &'static str> {
    match value {
        Value::String(name) if ty.enum_value(&name).is_some() => Ok(Value::String(name)),
        _ => Err("The output of enum resolver is not a defined value."),
    }
}

pub(crate) fn coerce_builtin_scalar(type_name: &str, value: Value) -> Result<Value, &'static str> {
    match (type_name, value) {
        ("Int", Value::Int(value)) if i32::try_from(value).is_ok() => Ok(Value::Int(value)),
        ("Float", Value::Float(value)) if value.is_finite() => Ok(Value::Float(value)),
        ("Float", Value::Int(value)) => crate::numeric::exact_float_from_integer(value)
            .map(Value::Float)
            .ok_or("Float result coercion would lose precision."),
        ("Float", Value::BigInt(raw)) if crate::numeric::valid_big_int_lexeme(&raw) => {
            crate::numeric::exact_float_from_integer_lexeme(&raw)
                .map(Value::Float)
                .ok_or("Float result coercion would lose precision.")
        }
        ("String", Value::String(value)) => Ok(Value::String(value)),
        ("Boolean", Value::Boolean(value)) => Ok(Value::Boolean(value)),
        ("ID", Value::String(value)) => Ok(Value::String(value)),
        ("ID", Value::Int(value)) => Ok(Value::String(value.to_string())),
        ("ID", Value::BigInt(raw)) if crate::numeric::valid_big_int_lexeme(&raw) => {
            Ok(Value::String(raw))
        }
        _ => Err("The type or range of built-in scalar resolver output is incorrect."),
    }
}
