//! Limited recursive-descent parser shared by SDL and executable document.

mod common;
mod cursor;
mod executable;
mod recursion;
mod type_system;
mod value;

use super::{Token, TokenKind, lex};
use crate::language::ast::Document;
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, ExecutionLimits, SourceId, Span};

/// GraphQL document has exceeded the set byte limit.
pub const PARSE_DOCUMENT_TOO_LARGE: DiagnosticCode = DiagnosticCode::new("SF1201");
/// The token at the current position is not the expected grammar element.
pub const PARSE_UNEXPECTED_TOKEN: DiagnosticCode = DiagnosticCode::new("SF1202");
/// The parser nesting limit is exceeded.
pub const PARSE_NESTING_LIMIT: DiagnosticCode = DiagnosticCode::new("SF1203");
/// The executable input is given type-system definition or extension.
pub const PARSE_TYPE_SYSTEM_DEFINITION: DiagnosticCode = DiagnosticCode::new("SF1204");
/// The SDL input is given an operation or fragment definition.
pub const PARSE_EXECUTABLE_DEFINITION: DiagnosticCode = DiagnosticCode::new("SF1205");

/// Document parse result and bounded diagnostics.
#[derive(Clone, Debug)]
pub struct ParseOutput {
    /// Parsed document, present only when no lexing or parsing error occurred.
    pub document: Option<Document>,
    /// Diagnostics retained in source order up to the configured limit.
    pub diagnostics: DiagnosticBag,
    /// Whether parsing or diagnostic retention stopped at the diagnostic limit.
    diagnostics_truncated: bool,
}

impl ParseOutput {
    /// Returns the complete document only when no errors occurred.
    pub const fn is_valid(&self) -> bool {
        self.document.is_some()
    }

    /// Returns whether parsing stopped at the diagnostic limit.
    pub const fn diagnostics_were_truncated(&self) -> bool {
        self.diagnostics_truncated
    }
}

pub(super) struct Parser {
    source_id: SourceId,
    tokens: Vec<Token>,
    position: usize,
    previous_end: usize,
    diagnostics: DiagnosticBag,
    had_errors: bool,
    diagnostic_limit_reached: bool,
    fatal_error: bool,
    nesting: usize,
    max_nesting: usize,
}

impl Parser {
    pub(super) fn new(
        source_id: SourceId,
        mut tokens: Vec<Token>,
        limits: &ExecutionLimits,
    ) -> Self {
        if !tokens.last().is_some_and(|token| token.kind.is_eof()) {
            let end = tokens.last().map_or(0, |token| token.span.end);
            tokens.push(Token::new(TokenKind::Eof, Span::new(source_id, end, end)));
        }
        Self {
            source_id,
            tokens,
            position: 0,
            previous_end: 0,
            diagnostics: DiagnosticBag::new(limits.max_diagnostics),
            had_errors: false,
            diagnostic_limit_reached: false,
            fatal_error: false,
            nesting: 0,
            max_nesting: limits.max_parser_nesting,
        }
    }

    pub(super) fn report(&mut self, code: DiagnosticCode, message: impl Into<String>, span: Span) {
        self.had_errors = true;
        let stored = self.diagnostics.push(Diagnostic::new(code, message, span));
        if !stored || self.diagnostics.is_full() {
            self.diagnostic_limit_reached = true;
        }
    }

    fn into_output(self, document: Document) -> ParseOutput {
        let valid = !self.had_errors && !self.fatal_error;
        ParseOutput {
            document: valid.then_some(document),
            diagnostics_truncated: self.diagnostic_limit_reached
                || self.diagnostics.was_truncated(),
            diagnostics: self.diagnostics,
        }
    }
}

/// Lexes and parses an executable document under explicit limits.
///
/// Lexical and syntax errors never expose partial tokens or a partial AST.
/// This is to prevent invalid input from moving on to subsequent validation or execution.
pub fn parse_executable_document(
    source_id: SourceId,
    source: &str,
    limits: &ExecutionLimits,
) -> ParseOutput {
    parse_with(source_id, source, limits, executable::parse_document)
}

/// Lexes and parses a type-system document under explicit limits.
///
/// The document containing the operation or fragment is not transferred to the SDL composition.
pub fn parse_type_system_document(
    source_id: SourceId,
    source: &str,
    limits: &ExecutionLimits,
) -> ParseOutput {
    parse_with(source_id, source, limits, parse_type_system_only)
}

/// Parses executable and type-system definitions in source order.
///
/// General tooling entry point; execution and composition use domain-specific parsers.
pub fn parse_document(source_id: SourceId, source: &str, limits: &ExecutionLimits) -> ParseOutput {
    parse_with(source_id, source, limits, parse_mixed_document)
}

fn parse_with(
    source_id: SourceId,
    source: &str,
    limits: &ExecutionLimits,
    parse: fn(&mut Parser) -> Document,
) -> ParseOutput {
    if source.len() > limits.max_document_bytes {
        return oversized_output(source_id, source.len(), limits.max_diagnostics);
    }
    let lexical = lex(source_id, source, limits.max_diagnostics);
    if lexical.had_errors {
        let truncated = lexical.diagnostics.was_truncated() || lexical.diagnostics.is_full();
        return ParseOutput {
            document: None,
            diagnostics: lexical.diagnostics,
            diagnostics_truncated: truncated,
        };
    }
    let mut parser = Parser::new(source_id, lexical.tokens, limits);
    let document = parse(&mut parser);
    parser.into_output(document)
}

fn parse_type_system_only(parser: &mut Parser) -> Document {
    parse_definitions(parser, DefinitionMode::TypeSystem)
}

fn parse_mixed_document(parser: &mut Parser) -> Document {
    parse_definitions(parser, DefinitionMode::Mixed)
}

#[derive(Clone, Copy)]
enum DefinitionMode {
    Mixed,
    TypeSystem,
}

fn parse_definitions(parser: &mut Parser, mode: DefinitionMode) -> Document {
    use crate::language::ast::Definition;

    let mut definitions = Vec::new();
    let mut first_start = None;
    while parser.can_continue() && !parser.at_eof() {
        let initial = parser.position();
        let description = parser.parse_description();
        let start = description
            .as_ref()
            .map_or_else(|| parser.current_start(), |value| value.span.start);
        first_start.get_or_insert(start);
        if executable_start(parser) {
            if matches!(mode, DefinitionMode::TypeSystem) {
                parser.report(
                    PARSE_EXECUTABLE_DEFINITION,
                    "Executable definitions cannot appear in a type-system document",
                    parser.peek().span,
                );
                consume_remaining_tokens(parser);
                break;
            }
            if let Some(definition) = executable::parse_definition(parser, description, start) {
                definitions.push(Definition::Executable(definition));
            }
        } else if type_system::at_definition_start(parser) {
            if let Some(definition) = type_system::parse_definition(parser, description) {
                definitions.push(Definition::TypeSystem(definition));
            }
        } else {
            parser.unexpected("a GraphQL definition");
        }
        parser.ensure_progress(initial);
    }
    if definitions.is_empty() && !parser.had_errors {
        parser.unexpected("a GraphQL definition");
    }
    let start = first_start.unwrap_or(0);
    Document {
        definitions,
        span: Span::new(parser.source_id, start, parser.last_end().max(start)),
    }
}

fn executable_start(parser: &Parser) -> bool {
    parser.at(&TokenKind::BraceLeft)
        || matches!(
            parser.peek_name(),
            Some("query" | "mutation" | "subscription" | "fragment")
        )
}

fn consume_remaining_tokens(parser: &mut Parser) {
    while !parser.at_eof() {
        parser.advance();
    }
}

fn oversized_output(source_id: SourceId, length: usize, max_diagnostics: usize) -> ParseOutput {
    let mut diagnostics = DiagnosticBag::new(max_diagnostics);
    let stored = diagnostics.push(Diagnostic::new(
        PARSE_DOCUMENT_TOO_LARGE,
        "GraphQL document exceeds the configured byte limit",
        Span::new(source_id, 0, length),
    ));
    ParseOutput {
        document: None,
        diagnostics_truncated: !stored || diagnostics.was_truncated(),
        diagnostics,
    }
}

#[cfg(test)]
mod tests_document;
#[cfg(test)]
mod tests_executable;
#[cfg(test)]
mod tests_grammar_edges;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_negative;
#[cfg(test)]
mod tests_security;
