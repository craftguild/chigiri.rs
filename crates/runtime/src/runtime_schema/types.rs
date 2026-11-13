//! Convert generated named-type descriptors to type-system AST nodes by kind.

use chigiri_core::language::ast::{
    EnumTypeDefinition, EnumValueDefinition, FieldDefinition, InputObjectTypeDefinition,
    InterfaceTypeDefinition, ObjectTypeDefinition, ScalarTypeDefinition, ScalarTypeExtension,
    TypeSystemDefinition, UnionTypeDefinition,
};

use crate::{BuiltInScalarOverlayDescriptor, TypeDescriptor, TypeDescriptorKind};

use super::{SpanCursor, directives, input};

pub(super) fn definition(
    value: &TypeDescriptor,
    cursor: &mut SpanCursor,
) -> Result<TypeSystemDefinition, String> {
    let span = cursor.span();
    let description = cursor.description(value.description);
    let name = cursor.name(value.name);
    let directives = directives::applications(&value.directives, cursor);
    Ok(match value.kind {
        TypeDescriptorKind::Scalar => TypeSystemDefinition::Scalar(ScalarTypeDefinition {
            description,
            name,
            directives,
            span,
        }),
        TypeDescriptorKind::Object => TypeSystemDefinition::Object(ObjectTypeDefinition {
            description,
            name,
            implements_interfaces: names(&value.interfaces, cursor),
            directives,
            fields: fields(value, cursor),
            span,
        }),
        TypeDescriptorKind::Interface => TypeSystemDefinition::Interface(InterfaceTypeDefinition {
            description,
            name,
            implements_interfaces: names(&value.interfaces, cursor),
            directives,
            fields: fields(value, cursor),
            span,
        }),
        TypeDescriptorKind::Union => TypeSystemDefinition::Union(UnionTypeDefinition {
            description,
            name,
            members: names(&value.possible_types, cursor),
            directives,
            span,
        }),
        TypeDescriptorKind::Enum => TypeSystemDefinition::Enum(EnumTypeDefinition {
            description,
            name,
            directives,
            values: value
                .enum_values
                .iter()
                .map(|item| {
                    let span = cursor.span();
                    EnumValueDefinition {
                        description: cursor.description(item.description),
                        name: cursor.name(item.name),
                        directives: directives::applications(&item.directives, cursor),
                        span,
                    }
                })
                .collect(),
            span,
        }),
        TypeDescriptorKind::InputObject => {
            TypeSystemDefinition::InputObject(InputObjectTypeDefinition {
                description,
                name,
                directives,
                fields: value
                    .input_fields
                    .iter()
                    .map(|field| input::input_value(field, cursor))
                    .collect(),
                span,
            })
        }
    })
}

pub(super) fn scalar_overlay(
    value: &BuiltInScalarOverlayDescriptor,
    cursor: &mut SpanCursor,
) -> Result<TypeSystemDefinition, String> {
    let span = cursor.span();
    Ok(TypeSystemDefinition::ScalarExtension(ScalarTypeExtension {
        name: cursor.name(value.name),
        directives: directives::applications(&value.directives, cursor),
        span,
    }))
}

fn fields(value: &TypeDescriptor, cursor: &mut SpanCursor) -> Vec<FieldDefinition> {
    value
        .fields
        .iter()
        .map(|field| {
            let span = cursor.span();
            FieldDefinition {
                description: cursor.description(field.description),
                name: cursor.name(field.name),
                arguments: field
                    .arguments
                    .iter()
                    .map(|argument| input::input_value(argument, cursor))
                    .collect(),
                ty: input::type_ref(&field.type_descriptor, cursor),
                directives: directives::applications(&field.directives, cursor),
                span,
            }
        })
        .collect()
}

fn names(
    values: &crate::DescriptorList<&'static str>,
    cursor: &mut SpanCursor,
) -> Vec<chigiri_core::language::ast::Name> {
    values.iter().map(|value| cursor.name(value)).collect()
}
