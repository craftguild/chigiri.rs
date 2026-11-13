//! Parse named type definitions by kind.

use super::super::Parser;
use super::{member, syntax_start};
use crate::language::TokenKind;
use crate::language::ast::{
    Description, EnumTypeDefinition, InputObjectTypeDefinition, InterfaceTypeDefinition,
    ObjectTypeDefinition, ScalarTypeDefinition, UnionTypeDefinition,
};

pub(super) fn parse_scalar(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<ScalarTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("scalar")?;
    let name = parser.parse_name("a scalar type name")?;
    let directives = parser.parse_directives(true);
    Some(ScalarTypeDefinition {
        description,
        name,
        directives,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_object(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<ObjectTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("type")?;
    let name = parser.parse_name("an object type name")?;
    let implements_interfaces = member::parse_implements_interfaces(parser)?;
    let directives = parser.parse_directives(true);
    let fields = if parser.at(&TokenKind::BraceLeft) {
        member::parse_fields(parser)?
    } else {
        Vec::new()
    };
    Some(ObjectTypeDefinition {
        description,
        name,
        implements_interfaces,
        directives,
        fields,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_interface(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<InterfaceTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("interface")?;
    let name = parser.parse_name("an interface type name")?;
    let implements_interfaces = member::parse_implements_interfaces(parser)?;
    let directives = parser.parse_directives(true);
    let fields = if parser.at(&TokenKind::BraceLeft) {
        member::parse_fields(parser)?
    } else {
        Vec::new()
    };
    Some(InterfaceTypeDefinition {
        description,
        name,
        implements_interfaces,
        directives,
        fields,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_union(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<UnionTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("union")?;
    let name = parser.parse_name("a union type name")?;
    let directives = parser.parse_directives(true);
    let members = if parser.at(&TokenKind::Equals) {
        member::parse_union_members(parser)?
    } else {
        Vec::new()
    };
    Some(UnionTypeDefinition {
        description,
        name,
        members,
        directives,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_enum(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<EnumTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("enum")?;
    let name = parser.parse_name("an enum type name")?;
    let directives = parser.parse_directives(true);
    let values = if parser.at(&TokenKind::BraceLeft) {
        member::parse_enum_values(parser)?
    } else {
        Vec::new()
    };
    Some(EnumTypeDefinition {
        description,
        name,
        directives,
        values,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_input_object(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<InputObjectTypeDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("input")?;
    let name = parser.parse_name("an input object type name")?;
    let directives = parser.parse_directives(true);
    let fields = if parser.at(&TokenKind::BraceLeft) {
        member::parse_input_fields(parser)?
    } else {
        Vec::new()
    };
    Some(InputObjectTypeDefinition {
        description,
        name,
        directives,
        fields,
        span: parser.span_from(start),
    })
}
