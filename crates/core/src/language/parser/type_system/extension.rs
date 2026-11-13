//! Builds kind-specific type-extension AST nodes and enforces their non-empty requirements.

use super::super::Parser;
use super::{member, schema};
use crate::language::TokenKind;
use crate::language::ast::{
    EnumTypeExtension, InputObjectTypeExtension, InterfaceTypeExtension, ObjectTypeExtension,
    ScalarTypeExtension, TypeSystemDefinition, UnionTypeExtension,
};

pub(super) fn parse(parser: &mut Parser) -> Option<TypeSystemDefinition> {
    let start = parser.current_start();
    parser.expect_keyword("extend")?;
    if parser.at_keyword("schema") {
        return schema::parse_extension_after_extend(parser, start)
            .map(TypeSystemDefinition::SchemaExtension);
    }
    if parser.at_keyword("scalar") {
        return parse_scalar(parser, start).map(TypeSystemDefinition::ScalarExtension);
    }
    if parser.at_keyword("type") {
        return parse_object(parser, start).map(TypeSystemDefinition::ObjectExtension);
    }
    if parser.at_keyword("interface") {
        return parse_interface(parser, start).map(TypeSystemDefinition::InterfaceExtension);
    }
    if parser.at_keyword("union") {
        return parse_union(parser, start).map(TypeSystemDefinition::UnionExtension);
    }
    if parser.at_keyword("enum") {
        return parse_enum(parser, start).map(TypeSystemDefinition::EnumExtension);
    }
    if parser.at_keyword("input") {
        return parse_input_object(parser, start).map(TypeSystemDefinition::InputObjectExtension);
    }
    parser.unexpected("a schema or named type kind after `extend`");
    None
}

fn parse_scalar(parser: &mut Parser, start: usize) -> Option<ScalarTypeExtension> {
    parser.expect_keyword("scalar")?;
    let name = parser.parse_name("a scalar type name")?;
    let directives = parser.parse_directives(true);
    if directives.is_empty() {
        parser.unexpected("one or more scalar extension directives");
    }
    Some(ScalarTypeExtension {
        name,
        directives,
        span: parser.span_from(start),
    })
}

fn parse_object(parser: &mut Parser, start: usize) -> Option<ObjectTypeExtension> {
    parser.expect_keyword("type")?;
    let name = parser.parse_name("an object type name")?;
    let implements_interfaces = member::parse_implements_interfaces(parser)?;
    let directives = parser.parse_directives(true);
    let has_fields = parser.at(&TokenKind::BraceLeft);
    let fields = if has_fields {
        member::parse_fields(parser)?
    } else {
        Vec::new()
    };
    require_object_extension_content(
        parser,
        !implements_interfaces.is_empty(),
        !directives.is_empty(),
        has_fields,
    );
    Some(ObjectTypeExtension {
        name,
        implements_interfaces,
        directives,
        fields,
        span: parser.span_from(start),
    })
}

fn parse_interface(parser: &mut Parser, start: usize) -> Option<InterfaceTypeExtension> {
    parser.expect_keyword("interface")?;
    let name = parser.parse_name("an interface type name")?;
    let implements_interfaces = member::parse_implements_interfaces(parser)?;
    let directives = parser.parse_directives(true);
    let has_fields = parser.at(&TokenKind::BraceLeft);
    let fields = if has_fields {
        member::parse_fields(parser)?
    } else {
        Vec::new()
    };
    require_object_extension_content(
        parser,
        !implements_interfaces.is_empty(),
        !directives.is_empty(),
        has_fields,
    );
    Some(InterfaceTypeExtension {
        name,
        implements_interfaces,
        directives,
        fields,
        span: parser.span_from(start),
    })
}

fn require_object_extension_content(
    parser: &mut Parser,
    has_interfaces: bool,
    has_directives: bool,
    has_fields: bool,
) {
    if !has_interfaces && !has_directives && !has_fields {
        parser.unexpected("implemented interfaces, directives, or field definitions");
    }
}

fn parse_union(parser: &mut Parser, start: usize) -> Option<UnionTypeExtension> {
    parser.expect_keyword("union")?;
    let name = parser.parse_name("a union type name")?;
    let directives = parser.parse_directives(true);
    let has_members = parser.at(&TokenKind::Equals);
    let members = if has_members {
        member::parse_union_members(parser)?
    } else {
        Vec::new()
    };
    if directives.is_empty() && !has_members {
        parser.unexpected("union member types or extension directives");
    }
    Some(UnionTypeExtension {
        name,
        members,
        directives,
        span: parser.span_from(start),
    })
}

fn parse_enum(parser: &mut Parser, start: usize) -> Option<EnumTypeExtension> {
    parser.expect_keyword("enum")?;
    let name = parser.parse_name("an enum type name")?;
    let directives = parser.parse_directives(true);
    let has_values = parser.at(&TokenKind::BraceLeft);
    let values = if has_values {
        member::parse_enum_values(parser)?
    } else {
        Vec::new()
    };
    if directives.is_empty() && !has_values {
        parser.unexpected("enum value definitions or extension directives");
    }
    Some(EnumTypeExtension {
        name,
        directives,
        values,
        span: parser.span_from(start),
    })
}

fn parse_input_object(parser: &mut Parser, start: usize) -> Option<InputObjectTypeExtension> {
    parser.expect_keyword("input")?;
    let name = parser.parse_name("an input object type name")?;
    let directives = parser.parse_directives(true);
    let has_fields = parser.at(&TokenKind::BraceLeft);
    let fields = if has_fields {
        member::parse_input_fields(parser)?
    } else {
        Vec::new()
    };
    if directives.is_empty() && !has_fields {
        parser.unexpected("input field definitions or extension directives");
    }
    Some(InputObjectTypeExtension {
        name,
        directives,
        fields,
        span: parser.span_from(start),
    })
}
