//! Convert generated directive definitions and applications to source-ordered AST nodes.

use chigiri_core::language::ast::{
    Argument, Directive, DirectiveDefinition, DirectiveLocation, Spanned,
};

use crate::{AppliedDirectiveDescriptor, DescriptorList, DirectiveDefinitionDescriptor};

use super::SpanCursor;
use super::input;

pub(super) fn applications(
    values: &DescriptorList<AppliedDirectiveDescriptor>,
    cursor: &mut SpanCursor,
) -> Vec<Directive> {
    values
        .iter()
        .map(|value| {
            let span = cursor.span();
            Directive {
                name: cursor.name(value.name),
                arguments: value
                    .arguments
                    .iter()
                    .map(|argument| {
                        let span = cursor.span();
                        Argument {
                            name: cursor.name(argument.name),
                            value: input::const_value(&argument.value_descriptor, cursor),
                            span,
                        }
                    })
                    .collect(),
                span,
            }
        })
        .collect()
}

pub(super) fn definition(
    value: &DirectiveDefinitionDescriptor,
    cursor: &mut SpanCursor,
) -> Result<DirectiveDefinition, String> {
    let span = cursor.span();
    let locations = value
        .locations
        .iter()
        .map(|value| {
            location(value)
                .map(|location| Spanned::new(location, cursor.span()))
                .ok_or_else(|| format!("unknown directive location `{value}`"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DirectiveDefinition {
        description: cursor.description(value.description),
        name: cursor.name(value.name),
        arguments: value
            .arguments
            .iter()
            .map(|argument| input::input_value(argument, cursor))
            .collect(),
        repeatable: value.repeatable.then(|| cursor.span()),
        locations,
        span,
    })
}

fn location(value: &str) -> Option<DirectiveLocation> {
    Some(match value {
        "QUERY" => DirectiveLocation::Query,
        "MUTATION" => DirectiveLocation::Mutation,
        "SUBSCRIPTION" => DirectiveLocation::Subscription,
        "FIELD" => DirectiveLocation::Field,
        "FRAGMENT_DEFINITION" => DirectiveLocation::FragmentDefinition,
        "FRAGMENT_SPREAD" => DirectiveLocation::FragmentSpread,
        "INLINE_FRAGMENT" => DirectiveLocation::InlineFragment,
        "VARIABLE_DEFINITION" => DirectiveLocation::VariableDefinition,
        "SCHEMA" => DirectiveLocation::Schema,
        "SCALAR" => DirectiveLocation::Scalar,
        "OBJECT" => DirectiveLocation::Object,
        "FIELD_DEFINITION" => DirectiveLocation::FieldDefinition,
        "ARGUMENT_DEFINITION" => DirectiveLocation::ArgumentDefinition,
        "INTERFACE" => DirectiveLocation::Interface,
        "UNION" => DirectiveLocation::Union,
        "ENUM" => DirectiveLocation::Enum,
        "ENUM_VALUE" => DirectiveLocation::EnumValue,
        "INPUT_OBJECT" => DirectiveLocation::InputObject,
        "INPUT_FIELD_DEFINITION" => DirectiveLocation::InputFieldDefinition,
        _ => return None,
    })
}
