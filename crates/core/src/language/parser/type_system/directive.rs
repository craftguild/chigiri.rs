//! Parse directive definitions, repeatability flags, and location lists.

use super::super::{PARSE_UNEXPECTED_TOKEN, Parser};
use super::{member, syntax_start};
use crate::language::TokenKind;
use crate::language::ast::{Description, DirectiveDefinition, DirectiveLocation, Spanned};

pub(super) fn parse(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<DirectiveDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("directive")?;
    parser.expect(&TokenKind::At, "`@`")?;
    let name = parser.parse_name("a directive name")?;
    let arguments = member::parse_arguments_definition(parser)?;
    let repeatable = parser.consume_keyword("repeatable").map(|token| token.span);
    parser.expect_keyword("on")?;
    let locations = parse_locations(parser)?;
    Some(DirectiveDefinition {
        description,
        name,
        arguments,
        repeatable,
        locations,
        span: parser.span_from(start),
    })
}

fn parse_locations(parser: &mut Parser) -> Option<Vec<Spanned<DirectiveLocation>>> {
    parser.consume(&TokenKind::Pipe);
    let mut locations = vec![parse_location(parser)?];
    while parser.consume(&TokenKind::Pipe).is_some() {
        locations.push(parse_location(parser)?);
    }
    Some(locations)
}

fn parse_location(parser: &mut Parser) -> Option<Spanned<DirectiveLocation>> {
    let name = parser.parse_name("a directive location")?;
    let location = match name.value.as_str() {
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
        _ => {
            parser.report(
                PARSE_UNEXPECTED_TOKEN,
                format!("Unknown directive location `{}`", name.value),
                name.span,
            );
            return None;
        }
    };
    Some(Spanned::new(location, name.span))
}
