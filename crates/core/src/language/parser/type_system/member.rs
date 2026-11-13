//! Shared grammar for type-system members, implementations, and union-member lists.

use super::super::{PARSE_UNEXPECTED_TOKEN, Parser};
use crate::language::TokenKind;
use crate::language::ast::{
    Description, EnumValueDefinition, FieldDefinition, InputValueDefinition, Name,
};

pub(super) fn parse_implements_interfaces(parser: &mut Parser) -> Option<Vec<Name>> {
    if parser.consume_keyword("implements").is_none() {
        return Some(Vec::new());
    }
    parser.consume(&TokenKind::Ampersand);
    let mut interfaces = vec![parser.parse_name("an implemented interface name")?];
    while parser.consume(&TokenKind::Ampersand).is_some() {
        interfaces.push(parser.parse_name("an implemented interface name")?);
    }
    Some(interfaces)
}

pub(super) fn parse_fields(parser: &mut Parser) -> Option<Vec<FieldDefinition>> {
    parse_braced_list(parser, "one or more field definitions", parse_field)
}

pub(super) fn parse_input_fields(parser: &mut Parser) -> Option<Vec<InputValueDefinition>> {
    parse_braced_list(
        parser,
        "one or more input field definitions",
        parse_input_value,
    )
}

pub(super) fn parse_enum_values(parser: &mut Parser) -> Option<Vec<EnumValueDefinition>> {
    parse_braced_list(
        parser,
        "one or more enum value definitions",
        parse_enum_value,
    )
}

fn parse_braced_list<T>(
    parser: &mut Parser,
    expected: &'static str,
    parse_item: fn(&mut Parser) -> Option<T>,
) -> Option<Vec<T>> {
    parser.nested(|parser| {
        parser.expect(&TokenKind::BraceLeft, "`{`")?;
        let mut items = Vec::new();
        if parser.at(&TokenKind::BraceRight) {
            parser.unexpected(expected);
        }
        while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::BraceRight) {
            let initial = parser.position();
            if let Some(item) = parse_item(parser) {
                items.push(item);
            }
            parser.ensure_progress(initial);
        }
        parser.expect(&TokenKind::BraceRight, "`}`")?;
        Some(items)
    })
}

fn parse_field(parser: &mut Parser) -> Option<FieldDefinition> {
    let description = parser.parse_description();
    let start = description_start(parser, &description);
    let name = parser.parse_name("a field name")?;
    let arguments = parse_arguments_definition(parser)?;
    parser.expect(&TokenKind::Colon, "`:`")?;
    let ty = parser.parse_type_ref()?;
    let directives = parser.parse_directives(true);
    Some(FieldDefinition {
        description,
        name,
        arguments,
        ty,
        directives,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_arguments_definition(parser: &mut Parser) -> Option<Vec<InputValueDefinition>> {
    if !parser.at(&TokenKind::ParenLeft) {
        return Some(Vec::new());
    }
    parser.nested(|parser| {
        parser.advance();
        let mut arguments = Vec::new();
        if parser.at(&TokenKind::ParenRight) {
            parser.unexpected("one or more argument definitions");
        }
        while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::ParenRight) {
            let initial = parser.position();
            if let Some(argument) = parse_input_value(parser) {
                arguments.push(argument);
            }
            parser.ensure_progress(initial);
        }
        parser.expect(&TokenKind::ParenRight, "`)`")?;
        Some(arguments)
    })
}

fn parse_input_value(parser: &mut Parser) -> Option<InputValueDefinition> {
    let description = parser.parse_description();
    let start = description_start(parser, &description);
    let name = parser.parse_name("an input value name")?;
    parser.expect(&TokenKind::Colon, "`:`")?;
    let ty = parser.parse_type_ref()?;
    let default_value = if parser.consume(&TokenKind::Equals).is_some() {
        parser.parse_const_value()
    } else {
        None
    };
    let directives = parser.parse_directives(true);
    Some(InputValueDefinition {
        description,
        name,
        ty,
        default_value,
        directives,
        span: parser.span_from(start),
    })
}

fn parse_enum_value(parser: &mut Parser) -> Option<EnumValueDefinition> {
    let description = parser.parse_description();
    let start = description_start(parser, &description);
    let name = parser.parse_name("an enum value name")?;
    if matches!(name.value.as_str(), "true" | "false" | "null") {
        parser.report(
            PARSE_UNEXPECTED_TOKEN,
            "Enum values cannot be `true`, `false`, or `null`",
            name.span,
        );
    }
    let directives = parser.parse_directives(true);
    Some(EnumValueDefinition {
        description,
        name,
        directives,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_union_members(parser: &mut Parser) -> Option<Vec<Name>> {
    parser.expect(&TokenKind::Equals, "`=`")?;
    parser.consume(&TokenKind::Pipe);
    let mut members = vec![parser.parse_name("a union member type name")?];
    while parser.consume(&TokenKind::Pipe).is_some() {
        members.push(parser.parse_name("a union member type name")?);
    }
    Some(members)
}

fn description_start(parser: &Parser, description: &Option<Description>) -> usize {
    description.as_ref().map_or_else(
        || parser.current_start(),
        |description| description.span.start,
    )
}
