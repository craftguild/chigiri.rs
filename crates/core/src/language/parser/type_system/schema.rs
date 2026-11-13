//! Root-operation grammar for explicit schema definitions and extensions.

use super::super::{PARSE_UNEXPECTED_TOKEN, Parser};
use super::syntax_start;
use crate::language::TokenKind;
use crate::language::ast::{
    Description, OperationType, RootOperationTypeDefinition, SchemaDefinition, SchemaExtension,
    Spanned,
};

pub(super) fn parse_definition(
    parser: &mut Parser,
    description: Option<Description>,
) -> Option<SchemaDefinition> {
    let start = syntax_start(parser, &description);
    parser.expect_keyword("schema")?;
    let directives = parser.parse_directives(true);
    let root_operations = parse_root_operations(parser)?;
    Some(SchemaDefinition {
        description,
        directives,
        root_operations,
        span: parser.span_from(start),
    })
}

pub(super) fn parse_extension_after_extend(
    parser: &mut Parser,
    start: usize,
) -> Option<SchemaExtension> {
    parser.expect_keyword("schema")?;
    let directives = parser.parse_directives(true);
    let root_operations = if parser.at(&TokenKind::BraceLeft) {
        parse_root_operations(parser)?
    } else {
        if directives.is_empty() {
            parser.unexpected("a schema directive or root operation definition");
        }
        Vec::new()
    };
    Some(SchemaExtension {
        directives,
        root_operations,
        span: parser.span_from(start),
    })
}

fn parse_root_operations(parser: &mut Parser) -> Option<Vec<RootOperationTypeDefinition>> {
    parser.nested(|parser| {
        parser.expect(&TokenKind::BraceLeft, "`{`")?;
        let mut roots = Vec::new();
        if parser.at(&TokenKind::BraceRight) {
            parser.unexpected("one or more root operation type definitions");
        }
        while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::BraceRight) {
            let initial = parser.position();
            if let Some(root) = parse_root_operation(parser) {
                roots.push(root);
            }
            parser.ensure_progress(initial);
        }
        parser.expect(&TokenKind::BraceRight, "`}`")?;
        Some(roots)
    })
}

fn parse_root_operation(parser: &mut Parser) -> Option<RootOperationTypeDefinition> {
    let start = parser.current_start();
    let token = parser.advance();
    let operation_type = match token.kind.name() {
        Some("query") => OperationType::Query,
        Some("mutation") => OperationType::Mutation,
        Some("subscription") => OperationType::Subscription,
        _ => {
            parser.report(
                PARSE_UNEXPECTED_TOKEN,
                "Expected `query`, `mutation`, or `subscription`",
                token.span,
            );
            return None;
        }
    };
    parser.expect(&TokenKind::Colon, "`:`")?;
    let named_type = parser.parse_name("a root object type name")?;
    Some(RootOperationTypeDefinition {
        operation_type: Spanned::new(operation_type, token.span),
        named_type,
        span: parser.span_from(start),
    })
}
