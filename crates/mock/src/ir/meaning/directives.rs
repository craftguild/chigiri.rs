//! Extract model names and field roles from syntax-validated directives.

use chigiri::core::language::ast::{Directive, Value, ValueNode};

use crate::schema::SchemaError;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Role {
    Plain,
    Relation,
    Manual,
}

pub(super) fn role(directives: &[Directive]) -> Result<Role, SchemaError> {
    let mut role = Role::Plain;
    for directive in directives {
        let next = match directive.name.value.as_str() {
            "relation" => Role::Relation,
            "virtual" => Role::Manual,
            _ => continue,
        };
        if role != Role::Plain {
            return Err(SchemaError::new(
                "conflicting field roles",
                Some(directive.span),
            ));
        }
        role = next;
    }
    Ok(role)
}

pub(super) fn find<'a>(directives: &'a [Directive], name: &str) -> Option<&'a Directive> {
    directives
        .iter()
        .find(|directive| directive.name.value == name)
}

pub(super) fn argument<'a>(directive: &'a Directive, name: &str) -> Option<&'a ValueNode> {
    directive
        .arguments
        .iter()
        .find(|argument| argument.name.value == name)
        .map(|argument| &argument.value)
}

pub(super) fn optional<'a>(
    directive: &'a Directive,
    name: &str,
) -> Result<Option<&'a str>, SchemaError> {
    match argument(directive, name).map(|value| &value.value) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.value.is_empty() => Ok(Some(&value.value)),
        _ => Err(SchemaError::new(
            "invalid directive name reference",
            Some(directive.span),
        )),
    }
}
