//! Validate closed state declarations without inferring types from values or Luau type strings.

use std::collections::BTreeSet;

use chigiri::core::Value;

use super::{StateFields, StateType};
use crate::value::{MAX_STRING_BYTES, ValueBudget, ValueError};

pub(super) fn fields(
    value: &Value,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<StateFields, ValueError> {
    budget.container(depth)?;
    let Value::Object(object) = value else {
        return Err(ValueError("state fields object required"));
    };
    let mut names = BTreeSet::new();
    let mut fields = StateFields::new();
    for (name, value) in object {
        name_check(name, &mut names, budget)?;
        fields.insert(name.to_owned(), kind(value, budget, depth + 1)?);
    }
    Ok(fields)
}

fn kind(value: &Value, budget: &mut ValueBudget, depth: usize) -> Result<StateType, ValueError> {
    budget.container(depth)?;
    let Value::Object(object) = value else {
        return Err(invalid());
    };
    let Some(Value::String(kind)) = object.get("kind") else {
        return Err(invalid());
    };
    let member = match kind.as_str() {
        "string" | "boolean" | "integer" | "number" => None,
        "object" => Some("fields"),
        "list" | "nullable" => Some("of"),
        _ => return Err(invalid()),
    };
    if object.len() != 1 + usize::from(member.is_some())
        || object
            .iter()
            .any(|(name, _)| name != "kind" && Some(name) != member)
    {
        return Err(invalid());
    }
    budget.string_node(kind.len(), depth + 1)?;
    budget.key("kind")?;
    if let Some(name) = member {
        budget.key(name)?;
    }
    Ok(match kind.as_str() {
        "string" => StateType::String,
        "boolean" => StateType::Boolean,
        "integer" => StateType::Integer,
        "number" => StateType::Number,
        "object" => StateType::Object {
            fields: fields(object.get("fields").ok_or_else(invalid)?, budget, depth + 1)?,
        },
        "list" => StateType::List {
            of: Box::new(self::kind(
                object.get("of").ok_or_else(invalid)?,
                budget,
                depth + 1,
            )?),
        },
        "nullable" => StateType::Nullable {
            of: Box::new(self::kind(
                object.get("of").ok_or_else(invalid)?,
                budget,
                depth + 1,
            )?),
        },
        _ => return Err(invalid()),
    })
}

pub(super) fn validate(
    fields: &StateFields,
    budget: &mut ValueBudget,
    depth: usize,
) -> Result<(), ValueError> {
    budget.container(depth)?;
    let mut names = BTreeSet::new();
    for (name, ty) in fields {
        name_check(name, &mut names, budget)?;
        validate_type(ty, budget, depth + 1)?;
    }
    Ok(())
}

fn validate_type(ty: &StateType, budget: &mut ValueBudget, depth: usize) -> Result<(), ValueError> {
    budget.container(depth)?;
    match ty {
        StateType::Object { fields } => validate(fields, budget, depth + 1),
        StateType::List { of } | StateType::Nullable { of } => validate_type(of, budget, depth + 1),
        _ => Ok(()),
    }
}

fn name_check(
    name: &str,
    names: &mut BTreeSet<String>,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    budget.key(name)?;
    if name.len() > MAX_STRING_BYTES
        || name.starts_with("__")
        || !name
            .bytes()
            .next()
            .is_some_and(|ch| ch == b'_' || ch.is_ascii_alphabetic())
        || !name
            .bytes()
            .all(|ch| ch == b'_' || ch.is_ascii_alphanumeric())
        || !names.insert(name.to_ascii_lowercase())
    {
        return Err(ValueError(
            "invalid, reserved or colliding state field name",
        ));
    }
    // Generated declarations quote Luau keywords, so user field names require no rewriting.
    Ok(())
}

fn invalid() -> ValueError {
    ValueError("closed state kind declaration required")
}
