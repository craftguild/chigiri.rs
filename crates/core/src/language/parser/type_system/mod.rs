//! Parses type-system definitions and extensions from GraphQL September 2025.

mod definition;
mod directive;
mod extension;
mod member;
mod schema;

use super::{PARSE_UNEXPECTED_TOKEN, Parser};
use crate::language::ast::{Description, TypeSystemDefinition};

/// Returns whether the current Name token starts type-system definition or extension.
pub(super) fn at_definition_start(parser: &Parser) -> bool {
    matches!(
        parser.peek_name(),
        Some(
            "schema"
                | "scalar"
                | "type"
                | "interface"
                | "union"
                | "enum"
                | "input"
                | "directive"
                | "extend"
        )
    )
}

/// Parses one type-system definition after consuming its optional description.
///
/// Descriptions are allowed only on definitions. A description before an
/// extension is recovered syntactically but causes the complete document to
/// fail through a diagnostic.
pub(super) fn parse_definition(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<TypeSystemDefinition> {
    if parser.at_keyword("extend") {
        if let Some(description) = description {
            parser.report(
                PARSE_UNEXPECTED_TOKEN,
                "Description is not allowed before a type-system extension",
                description.span,
            );
        }
        return extension::parse(parser);
    }

    if parser.at_keyword("schema") {
        return schema::parse_definition(parser, description).map(TypeSystemDefinition::Schema);
    }
    if parser.at_keyword("scalar") {
        return definition::parse_scalar(parser, description).map(TypeSystemDefinition::Scalar);
    }
    if parser.at_keyword("type") {
        return definition::parse_object(parser, description).map(TypeSystemDefinition::Object);
    }
    if parser.at_keyword("interface") {
        return definition::parse_interface(parser, description)
            .map(TypeSystemDefinition::Interface);
    }
    if parser.at_keyword("union") {
        return definition::parse_union(parser, description).map(TypeSystemDefinition::Union);
    }
    if parser.at_keyword("enum") {
        return definition::parse_enum(parser, description).map(TypeSystemDefinition::Enum);
    }
    if parser.at_keyword("input") {
        return definition::parse_input_object(parser, description)
            .map(TypeSystemDefinition::InputObject);
    }
    if parser.at_keyword("directive") {
        return directive::parse(parser, description).map(TypeSystemDefinition::Directive);
    }

    parser.unexpected("a type-system definition");
    None
}

pub(super) fn syntax_start(parser: &Parser, description: &Option<Description>) -> usize {
    description.as_ref().map_or_else(
        || parser.current_start(),
        |description| description.span.start,
    )
}

#[cfg(test)]
mod tests_definition;
#[cfg(test)]
mod tests_description;
#[cfg(test)]
mod tests_extension;
#[cfg(test)]
mod tests_rejection;
#[cfg(test)]
mod tests_support;
