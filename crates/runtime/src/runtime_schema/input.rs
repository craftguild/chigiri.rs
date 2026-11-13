//! Convert descriptor type references, constant values, and input definitions to AST nodes.

use chigiri_core::language::ast::{
    InputValueDefinition, ObjectField, StringStyle, StringValue, TypeRef, Value as AstValue,
    ValueNode,
};

use crate::{ConstValueDescriptor, InputValueDescriptor, TypeRefDescriptor};

use super::{SpanCursor, directives};

pub(super) fn input_value(
    value: &InputValueDescriptor,
    cursor: &mut SpanCursor,
) -> InputValueDefinition {
    let span = cursor.span();
    InputValueDefinition {
        description: cursor.description(value.description),
        name: cursor.name(value.name),
        ty: type_ref(&value.type_descriptor, cursor),
        default_value: value
            .default_descriptor
            .as_ref()
            .map(|value| const_value(value, cursor)),
        directives: directives::applications(&value.directives, cursor),
        span,
    }
}

pub(super) fn type_ref(value: &TypeRefDescriptor, cursor: &mut SpanCursor) -> TypeRef {
    let span = cursor.span();
    match value {
        TypeRefDescriptor::Named(name) => TypeRef::named(cursor.name(name), span),
        TypeRefDescriptor::List(item) => TypeRef::list(type_ref(item, cursor), span),
        TypeRefDescriptor::NonNull(inner) => TypeRef::non_null(type_ref(inner, cursor), span),
    }
}

pub(super) fn const_value(value: &ConstValueDescriptor, cursor: &mut SpanCursor) -> ValueNode {
    let span = cursor.span();
    let value = match value {
        ConstValueDescriptor::Int(value) => AstValue::Int((*value).to_owned()),
        ConstValueDescriptor::Float(value) => AstValue::Float((*value).to_owned()),
        ConstValueDescriptor::String(value) => {
            AstValue::String(StringValue::new(*value, StringStyle::Quoted))
        }
        ConstValueDescriptor::Boolean(value) => AstValue::Boolean(*value),
        ConstValueDescriptor::Null => AstValue::Null,
        ConstValueDescriptor::Enum(value) => AstValue::Enum(cursor.name(value)),
        ConstValueDescriptor::List(values) => AstValue::List(
            values
                .iter()
                .map(|value| const_value(value, cursor))
                .collect(),
        ),
        ConstValueDescriptor::Object(fields) => AstValue::Object(
            fields
                .iter()
                .map(|field| {
                    let span = cursor.span();
                    ObjectField {
                        name: cursor.name(field.name),
                        value: const_value(&field.value, cursor),
                        span,
                    }
                })
                .collect(),
        ),
    };
    ValueNode::new(value, span)
}
