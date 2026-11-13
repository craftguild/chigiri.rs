//! Executable grammar from GraphQL September 2025 §§ 2.3–2.13.

use super::{PARSE_TYPE_SYSTEM_DEFINITION, Parser, type_system};
use crate::Span;
use crate::language::TokenKind;
use crate::language::ast::VariableDefinition;
use crate::language::ast::{Definition, Description, Document, ExecutableDefinition, Field};
use crate::language::ast::{FragmentDefinition, FragmentSpread, InlineFragment, Name};
use crate::language::ast::{OperationDefinition, OperationType, Selection, SelectionSet};

pub(super) fn parse_document(parser: &mut Parser) -> Document {
    let mut definitions = Vec::new();
    let mut first_start = None;
    while parser.can_continue() && !parser.at_eof() {
        let initial = parser.position();
        let description = parser.parse_description();
        let start = description
            .as_ref()
            .map_or_else(|| parser.current_start(), |value| value.span.start);
        first_start.get_or_insert(start);
        if type_system::at_definition_start(parser) {
            parser.report(
                PARSE_TYPE_SYSTEM_DEFINITION,
                "Type-system definitions cannot appear in an executable document",
                parser.peek().span,
            );
            consume_remaining_tokens(parser);
            break;
        }
        if let Some(definition) = parse_definition(parser, description, start) {
            definitions.push(Definition::Executable(definition));
        }
        parser.ensure_progress(initial);
    }
    if definitions.is_empty() && !parser.had_errors {
        parser.unexpected("an executable definition");
    }
    let start = first_start.unwrap_or(0);
    Document {
        definitions,
        span: Span::new(parser.source_id, start, parser.last_end().max(start)),
    }
}

pub(super) fn parse_definition(
    parser: &mut Parser,
    description: Option<Description>,
    start: usize,
) -> Option<ExecutableDefinition> {
    if parser.at(&TokenKind::BraceLeft) {
        if description.is_some() {
            parser.unexpected("an explicit operation after a description");
        }
        return parse_shorthand_operation(parser, start).map(ExecutableDefinition::Operation);
    }
    if matches!(
        parser.peek_name(),
        Some("query" | "mutation" | "subscription")
    ) {
        return parse_explicit_operation(parser, description, start)
            .map(ExecutableDefinition::Operation);
    }
    if parser.at_keyword("fragment") {
        return parse_fragment_definition(parser, description, start)
            .map(ExecutableDefinition::Fragment);
    }
    parser.unexpected("an operation or fragment definition");
    None
}

fn parse_shorthand_operation(parser: &mut Parser, start: usize) -> Option<OperationDefinition> {
    let selection_set = parse_selection_set(parser)?;
    Some(OperationDefinition {
        description: None,
        operation_type: OperationType::Query,
        operation_type_span: None,
        name: None,
        variable_definitions: Vec::new(),
        directives: Vec::new(),
        selection_set,
        span: parser.span_from(start),
    })
}

fn parse_explicit_operation(
    parser: &mut Parser,
    description: Option<Description>,
    start: usize,
) -> Option<OperationDefinition> {
    let operation_token = parser.advance();
    let operation_type = match operation_token.kind.name()? {
        "query" => OperationType::Query,
        "mutation" => OperationType::Mutation,
        "subscription" => OperationType::Subscription,
        _ => return None,
    };
    let name = parser
        .peek_name()
        .is_some()
        .then(|| parser.parse_name("an operation name"))
        .flatten();
    let variable_definitions = parse_variable_definitions(parser);
    let directives = parser.parse_directives(false);
    let selection_set = parse_selection_set(parser)?;
    Some(OperationDefinition {
        description,
        operation_type,
        operation_type_span: Some(operation_token.span),
        name,
        variable_definitions,
        directives,
        selection_set,
        span: parser.span_from(start),
    })
}

fn parse_variable_definitions(parser: &mut Parser) -> Vec<VariableDefinition> {
    if parser.consume(&TokenKind::ParenLeft).is_none() {
        return Vec::new();
    }
    let mut definitions = Vec::new();
    if parser.at(&TokenKind::ParenRight) {
        parser.unexpected("one or more variable definitions");
    }
    while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::ParenRight) {
        let initial = parser.position();
        if let Some(definition) = parse_variable_definition(parser) {
            definitions.push(definition);
        }
        parser.ensure_progress(initial);
    }
    parser.expect(&TokenKind::ParenRight, "`)`");
    definitions
}

fn parse_variable_definition(parser: &mut Parser) -> Option<VariableDefinition> {
    let description = parser.parse_description();
    let start = description
        .as_ref()
        .map_or_else(|| parser.current_start(), |value| value.span.start);
    parser.expect(&TokenKind::Dollar, "`$`")?;
    let variable = parser.parse_name("a variable name")?;
    parser.expect(&TokenKind::Colon, "`:`")?;
    let ty = parser.parse_type_ref()?;
    let default_value = if parser.consume(&TokenKind::Equals).is_some() {
        parser.parse_const_value()
    } else {
        None
    };
    let directives = parser.parse_directives(true);
    Some(VariableDefinition {
        description,
        variable,
        ty,
        default_value,
        directives,
        span: parser.span_from(start),
    })
}

fn parse_fragment_definition(
    parser: &mut Parser,
    description: Option<Description>,
    start: usize,
) -> Option<FragmentDefinition> {
    parser.expect_keyword("fragment")?;
    let name = parser.parse_name("a fragment name")?;
    if name.value == "on" {
        parser.report(
            super::PARSE_UNEXPECTED_TOKEN,
            "Fragment name cannot be `on`",
            name.span,
        );
    }
    parser.expect_keyword("on")?;
    let type_condition = parser.parse_name("a fragment type condition")?;
    let directives = parser.parse_directives(false);
    let selection_set = parse_selection_set(parser)?;
    Some(FragmentDefinition {
        description,
        name,
        type_condition,
        directives,
        selection_set,
        span: parser.span_from(start),
    })
}

fn parse_selection_set(parser: &mut Parser) -> Option<SelectionSet> {
    let start = parser.current_start();
    parser.nested(|parser| {
        parser.expect(&TokenKind::BraceLeft, "`{`")?;
        let mut selections = Vec::new();
        if parser.at(&TokenKind::BraceRight) {
            parser.unexpected("one or more selections");
        }
        while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::BraceRight) {
            let initial = parser.position();
            if let Some(selection) = parse_selection(parser) {
                selections.push(selection);
            }
            parser.ensure_progress(initial);
        }
        parser.expect(&TokenKind::BraceRight, "`}`")?;
        Some(SelectionSet {
            selections,
            span: parser.span_from(start),
        })
    })
}

fn parse_selection(parser: &mut Parser) -> Option<Selection> {
    if parser.at(&TokenKind::Spread) {
        return parse_fragment_selection(parser);
    }
    if parser.peek_name().is_some() {
        return parse_field(parser).map(Selection::Field);
    }
    parser.unexpected("a field or fragment selection");
    None
}

fn parse_field(parser: &mut Parser) -> Option<Field> {
    let start = parser.current_start();
    let first_name = parser.parse_name("a field name")?;
    let (alias, name) = if parser.consume(&TokenKind::Colon).is_some() {
        (Some(first_name), parser.parse_name("a field name")?)
    } else {
        (None, first_name)
    };
    let arguments = parser.parse_arguments(false);
    let directives = parser.parse_directives(false);
    let selection_set = parser
        .at(&TokenKind::BraceLeft)
        .then(|| parse_selection_set(parser))
        .flatten();
    Some(Field {
        alias,
        name,
        arguments,
        directives,
        selection_set,
        span: parser.span_from(start),
    })
}

fn parse_fragment_selection(parser: &mut Parser) -> Option<Selection> {
    let start = parser.current_start();
    parser.advance();
    if parser.consume_keyword("on").is_some() {
        let condition = Some(parser.parse_name("a fragment type condition")?);
        return parse_inline_fragment(parser, start, condition);
    }
    if parser.at(&TokenKind::At) || parser.at(&TokenKind::BraceLeft) {
        return parse_inline_fragment(parser, start, None);
    }
    let name = parser.parse_name("a fragment name")?;
    let directives = parser.parse_directives(false);
    Some(Selection::FragmentSpread(FragmentSpread {
        name,
        directives,
        span: parser.span_from(start),
    }))
}

fn parse_inline_fragment(
    parser: &mut Parser,
    start: usize,
    type_condition: Option<Name>,
) -> Option<Selection> {
    let directives = parser.parse_directives(false);
    let selection_set = parse_selection_set(parser)?;
    Some(Selection::InlineFragment(InlineFragment {
        type_condition,
        directives,
        selection_set,
        span: parser.span_from(start),
    }))
}

fn consume_remaining_tokens(parser: &mut Parser) {
    // The entire document is invalid in the executable input, so the rest is discarded in a non-return loop.
    while !parser.at_eof() {
        parser.advance();
    }
}
