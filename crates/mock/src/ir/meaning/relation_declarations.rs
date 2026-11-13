//! Register explicit relation candidates while separating names, arity, and direction from syntax.

use std::{collections::BTreeSet, sync::Arc};

use chigiri::core::language::ast::{Directive, Value};

use super::{Build, directives};
use crate::ir::{FieldId, ModelId};
use crate::schema::SchemaError;

pub(super) struct Declaration {
    pub field: FieldId,
    pub owner: ModelId,
    pub target: ModelId,
    pub name: Option<Arc<str>>,
    pub fields: Vec<Arc<str>>,
    pub references: Vec<Arc<str>>,
    pub list: bool,
}

pub(super) fn collect(build: &Build) -> Result<Vec<Declaration>, SchemaError> {
    let mut declarations = Vec::new();
    for (index, field) in build.graph.fields().iter().enumerate() {
        let id = FieldId(index);
        let Some(directive) = directives::find(
            &build.graph.field_source(id).unwrap().directives,
            "relation",
        ) else {
            continue;
        };
        let owner = build
            .model_types
            .get(&field.parent)
            .copied()
            .ok_or_else(|| build.error(id, "relation requires a concrete stored model"))?;
        let target = build
            .model_types
            .get(&field.ty.named)
            .copied()
            .ok_or_else(|| build.error(id, "relation target must be a concrete model"))?;
        if field.ty.list_depth() > 1 || !field.arguments.is_empty() {
            return Err(build.error(id, "invalid relation shape or arguments"));
        }
        let fields = names(directive, "fields")?;
        let references = names(directive, "references")?;
        if fields.is_some() != references.is_some()
            || fields.as_ref().map(Vec::len) != references.as_ref().map(Vec::len)
        {
            return Err(build.error(id, "relation fields and references must correspond"));
        }
        let name = directives::optional(directive, "name")?;
        if name.is_some_and(|name| name.contains('\0')) {
            return Err(build.error(id, "relation name cannot contain NUL"));
        }
        declarations.push(Declaration {
            field: id,
            owner,
            target,
            name: name.map(Arc::from),
            fields: fields.unwrap_or_default(),
            references: references.unwrap_or_default(),
            list: field.ty.list_depth() == 1,
        });
    }
    Ok(declarations)
}

fn names(directive: &Directive, name: &str) -> Result<Option<Vec<Arc<str>>>, SchemaError> {
    let error = || SchemaError::new("invalid relation component names", Some(directive.span));
    let Some(value) = directives::argument(directive, name) else {
        return Ok(None);
    };
    let values = match &value.value {
        Value::Null => return Ok(None),
        Value::List(values) => values.as_slice(),
        // Just like the list input coercion in GraphQL, we treat each string as one component.
        Value::String(_) => std::slice::from_ref(value),
        _ => return Err(error()),
    };
    if values.is_empty() {
        return Err(error());
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let Value::String(text) = &value.value else {
            return Err(error());
        };
        let name = text.value.as_str();
        let mut bytes = name.bytes();
        if !bytes
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            || !bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || name.starts_with("__")
            || !seen.insert(name)
        {
            return Err(error());
        }
        result.push(Arc::from(name));
    }
    Ok(Some(result))
}
