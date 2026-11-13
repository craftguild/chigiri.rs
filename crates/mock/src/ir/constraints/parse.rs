//! Parse field constraints and custom-scalar base constraints at the SDL boundary.

use std::sync::Arc;

use chigiri::core::Span;
use chigiri::core::language::ast::{Directive, Value, ValueNode};

use super::{Constraints, Format, PatternRegistry, Representation, domains};
use crate::ir::{TypeNode, TypeRef};
use crate::schema::SchemaError;

impl Constraints {
    pub(crate) fn field(
        types: &[TypeNode],
        ty: &TypeRef,
        directives: &[Directive],
        patterns: &mut PatternRegistry,
    ) -> Result<Self, SchemaError> {
        let Some(directive) = directives.iter().find(|d| d.name.value == "constraint") else {
            return Ok(Self::default());
        };
        let text = |name| string(directive, name);
        let pattern = text("pattern")?
            .map(|source| patterns.intern(&source, directive.span))
            .transpose()?;
        let format = text("format")?
            .map(|name| Format::parse(&name).ok_or_else(|| invalid(directive.span)))
            .transpose()?;
        let result = Self {
            min: number(directive, "min")?,
            max: number(directive, "max")?,
            exclusive_min: number(directive, "exclusiveMin")?,
            exclusive_max: number(directive, "exclusiveMax")?,
            multiple_of: number(directive, "multipleOf")?,
            min_length: length(directive, "minLength")?,
            max_length: length(directive, "maxLength")?,
            starts_with: text("startsWith")?,
            ends_with: text("endsWith")?,
            contains: text("contains")?,
            not_contains: text("notContains")?,
            pattern,
            format,
            min_items: length(directive, "minItems")?,
            max_items: length(directive, "maxItems")?,
            ..Self::default()
        };
        let named = &types[ty.named.0];
        let shape = named
            .scalar
            .as_ref()
            .map(|s| s.representation)
            .or(match named.name.as_ref() {
                "Int" => Some(Representation::Integer),
                "Float" => Some(Representation::Number),
                "ID" | "String" => Some(Representation::String),
                _ => None,
            });
        domains::validate(
            &result,
            named,
            ty,
            shape,
            directive.span,
            &mut patterns.work,
        )?;
        Ok(result)
    }

    pub(super) fn basic(directive: &Directive) -> Result<Self, SchemaError> {
        Ok(Self {
            min: number(directive, "min")?,
            max: number(directive, "max")?,
            min_bytes: length(directive, "minBytes")?,
            max_bytes: length(directive, "maxBytes")?,
            min_items: length(directive, "minItems")?,
            max_items: length(directive, "maxItems")?,
            ..Self::default()
        })
    }
}

pub(super) fn argument<'a>(directive: &'a Directive, name: &str) -> Option<&'a ValueNode> {
    directive
        .arguments
        .iter()
        .find(|arg| arg.name.value == name)
        .map(|arg| &arg.value)
        .filter(|value| !matches!(value.value, Value::Null))
}

fn number(directive: &Directive, name: &str) -> Result<Option<f64>, SchemaError> {
    let Some(value) = argument(directive, name) else {
        return Ok(None);
    };
    let raw = match &value.value {
        Value::Int(raw) | Value::Float(raw) => raw,
        _ => return Err(invalid(value.span)),
    };
    raw.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(Some)
        .ok_or_else(|| invalid(value.span))
}

fn length(directive: &Directive, name: &str) -> Result<Option<usize>, SchemaError> {
    let Some(value) = argument(directive, name) else {
        return Ok(None);
    };
    let Value::Int(raw) = &value.value else {
        return Err(invalid(value.span));
    };
    raw.parse::<i32>()
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .map(Some)
        .ok_or_else(|| invalid(value.span))
}

fn string(directive: &Directive, name: &str) -> Result<Option<Arc<str>>, SchemaError> {
    let Some(value) = argument(directive, name) else {
        return Ok(None);
    };
    let Value::String(text) = &value.value else {
        return Err(invalid(value.span));
    };
    if text.value.len() > crate::value::MAX_STRING_BYTES {
        return Err(invalid(value.span));
    }
    Ok(Some(Arc::from(text.value.as_str())))
}

pub(super) fn invalid(span: Span) -> SchemaError {
    SchemaError::new("invalid value constraints", Some(span))
}
