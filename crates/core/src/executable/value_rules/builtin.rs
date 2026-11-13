//! Determine static compatibility of built-in scalar literals.

use crate::language::ast::Value as AstValue;

pub(super) fn is_builtin(name: &str) -> bool {
    matches!(name, "Int" | "Float" | "String" | "Boolean" | "ID")
}

pub(super) fn valid(name: &str, value: &AstValue) -> bool {
    match (name, value) {
        ("Int", AstValue::Int(raw)) => raw.parse::<i32>().is_ok(),
        ("Float", AstValue::Int(raw)) => {
            crate::numeric::exact_float_from_integer_lexeme(raw).is_some()
        }
        ("Float", AstValue::Float(raw)) => crate::numeric::finite_float_from_lexeme(raw).is_some(),
        ("String", AstValue::String(_)) | ("Boolean", AstValue::Boolean(_)) => true,
        ("ID", AstValue::String(_)) => true,
        ("ID", AstValue::Int(raw)) => {
            crate::numeric::canonical_id_from_integer_lexeme(raw).is_some()
        }
        _ => false,
    }
}
