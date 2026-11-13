//! Render IR wrappers, constraints, and defaults without numeric rounding or information loss.

use super::{ReferenceError, schema_types::*};
use crate::ir::{Constraints, MockIr, Representation, ScalarSpec, TypeRef, Wrapper};
use chigiri::core::language::ast::{Description, Value, ValueNode};

pub(super) fn id(kind: &str, index: usize) -> String {
    format!("{kind}/{index}")
}

pub(super) fn description(ir: &MockIr, value: Option<&Description>) -> Option<DescriptionDoc> {
    let value = value?;
    let source = ir
        .graph()
        .source()
        .sources()
        .get(value.span.source_id.index())?;
    if source.name() == crate::schema::VOCABULARY_SOURCE {
        return None;
    }
    Some(DescriptionDoc {
        origin: DescriptionOrigin::UserSdl,
        text: value.value.value.clone(),
    })
}

pub(super) fn reference(ty: &TypeRef) -> TypeRefDoc {
    TypeRefDoc {
        type_id: id("type", ty.named.0),
        wrappers: ty
            .wrappers
            .iter()
            .map(|wrapper| match wrapper {
                Wrapper::NonNull => WrapperDoc::NonNull,
                Wrapper::List => WrapperDoc::List,
            })
            .collect(),
    }
}

pub(super) fn constraints(value: &Constraints) -> ConstraintsDoc {
    ConstraintsDoc {
        exclusive_min: value.exclusive_min,
        exclusive_max: value.exclusive_max,
        multiple_of: value.multiple_of,
        min_length: value.min_length,
        max_length: value.max_length,
        starts_with: value.starts_with.as_ref().map(ToString::to_string),
        ends_with: value.ends_with.as_ref().map(ToString::to_string),
        contains: value.contains.as_ref().map(ToString::to_string),
        not_contains: value.not_contains.as_ref().map(ToString::to_string),
        pattern: value
            .pattern
            .as_ref()
            .map(|pattern| pattern.source().to_owned()),
        format: value.format,
        min: value.min,
        max: value.max,
        min_bytes: value.min_bytes,
        max_bytes: value.max_bytes,
        min_items: value.min_items,
        max_items: value.max_items,
    }
}

pub(super) fn scalar(value: &ScalarSpec) -> ScalarDoc {
    ScalarDoc {
        representation: match value.representation {
            Representation::String => RepresentationDoc::String,
            Representation::Boolean => RepresentationDoc::Boolean,
            Representation::Integer => RepresentationDoc::Integer,
            Representation::Number => RepresentationDoc::Number,
            Representation::Object => RepresentationDoc::Object,
            Representation::Array => RepresentationDoc::Array,
            Representation::Json => RepresentationDoc::Json,
        },
        constraints: constraints(&value.constraints),
    }
}

pub(super) fn literal(value: &ValueNode) -> Result<LiteralDoc, ReferenceError> {
    literal_value(&value.value)
}

pub(super) fn literal_value(value: &Value) -> Result<LiteralDoc, ReferenceError> {
    Ok(match value {
        Value::Null => LiteralDoc::Null,
        Value::Boolean(value) => LiteralDoc::Boolean(*value),
        Value::Int(value) => LiteralDoc::Integer(value.clone()),
        Value::Float(value) => LiteralDoc::Float(value.clone()),
        Value::String(value) => LiteralDoc::String(value.value.clone()),
        Value::Enum(value) => LiteralDoc::Enum(value.value.clone()),
        Value::List(values) => LiteralDoc::List(
            values
                .iter()
                .map(|value| literal_value(&value.value))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(values) => LiteralDoc::Object(
            values
                .iter()
                .map(|field| {
                    Ok(LiteralFieldDoc {
                        name: field.name.value.clone(),
                        value: literal_value(&field.value.value)?,
                    })
                })
                .collect::<Result<_, ReferenceError>>()?,
        ),
        Value::Variable(_) => return Err(ReferenceError::contract()),
    })
}
