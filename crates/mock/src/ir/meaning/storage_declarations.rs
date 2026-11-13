//! Close storage SDL to private definitions and stored object fields.

use chigiri::core::language::{ast::TypeSystemDefinition, parse_type_system_document};
use chigiri::core::{ExecutionLimits, Source, SourceId, Span};

use crate::schema::SchemaError;

pub(super) fn validate(storage: &[Source], public_count: usize) -> Result<(), SchemaError> {
    for (index, source) in storage.iter().enumerate() {
        let parsed = parse_type_system_document(
            SourceId::new(public_count + index),
            source.text(),
            &ExecutionLimits::production(),
        );
        let Some(document) = parsed.document else {
            continue;
        };
        for definition in document.type_system_definitions() {
            match definition {
                TypeSystemDefinition::Object(_)
                | TypeSystemDefinition::Enum(_)
                | TypeSystemDefinition::Scalar(_) => {}
                TypeSystemDefinition::ObjectExtension(extension)
                    if extension.implements_interfaces.is_empty()
                        && extension.directives.is_empty()
                        && !extension.fields.is_empty() => {}
                TypeSystemDefinition::Schema(schema) => {
                    return root_error(
                        schema
                            .root_operations
                            .first()
                            .map_or(schema.span, |root| root.span),
                    );
                }
                TypeSystemDefinition::SchemaExtension(schema) => {
                    return root_error(
                        schema
                            .root_operations
                            .first()
                            .map_or(schema.span, |root| root.span),
                    );
                }
                other => {
                    return Err(SchemaError::new(
                        "unsupported storage declaration",
                        Some(span(other)),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn root_error(span: Span) -> Result<(), SchemaError> {
    Err(SchemaError::new(
        "storage operation root is not allowed",
        Some(span),
    ))
}

fn span(definition: &TypeSystemDefinition) -> Span {
    match definition {
        TypeSystemDefinition::Schema(value) => value.span,
        TypeSystemDefinition::Scalar(value) => value.span,
        TypeSystemDefinition::Object(value) => value.span,
        TypeSystemDefinition::Interface(value) => value.span,
        TypeSystemDefinition::Union(value) => value.span,
        TypeSystemDefinition::Enum(value) => value.span,
        TypeSystemDefinition::InputObject(value) => value.span,
        TypeSystemDefinition::Directive(value) => value.span,
        TypeSystemDefinition::SchemaExtension(value) => value.span,
        TypeSystemDefinition::ScalarExtension(value) => value.span,
        TypeSystemDefinition::ObjectExtension(value) => value.span,
        TypeSystemDefinition::InterfaceExtension(value) => value.span,
        TypeSystemDefinition::UnionExtension(value) => value.span,
        TypeSystemDefinition::EnumExtension(value) => value.span,
        TypeSystemDefinition::InputObjectExtension(value) => value.span,
    }
}
