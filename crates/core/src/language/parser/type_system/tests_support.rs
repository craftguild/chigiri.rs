use super::super::Parser;
use super::parse_definition;
use crate::language::ast::TypeSystemDefinition;
use crate::language::lex;
use crate::{Diagnostic, ExecutionLimits, SourceId};

pub(super) struct Parsed {
    pub definitions: Vec<TypeSystemDefinition>,
    pub diagnostics: Vec<Diagnostic>,
    pub at_eof: bool,
}

pub(super) fn parse(source: &str) -> Parsed {
    parse_with_limits(source, &ExecutionLimits::production())
}

pub(super) fn parse_with_limits(source: &str, limits: &ExecutionLimits) -> Parsed {
    let source_id = SourceId::new(42);
    let lexical = lex(source_id, source, 100);
    assert!(lexical.is_valid(), "test source must be lexically valid.");
    let mut parser = Parser::new(source_id, lexical.tokens, limits);
    let mut definitions = Vec::new();
    while parser.can_continue() && !parser.at_eof() {
        let initial = parser.position();
        let description = parser.parse_description();
        if let Some(definition) = parse_definition(&mut parser, description) {
            definitions.push(definition);
        }
        parser.ensure_progress(initial);
    }
    let at_eof = parser.at_eof();
    Parsed {
        definitions,
        diagnostics: parser.diagnostics.into_vec(),
        at_eof,
    }
}

pub(super) fn assert_valid(source: &str) -> Vec<TypeSystemDefinition> {
    let parsed = parse(source);
    assert!(parsed.at_eof, "parser must consume the complete document");
    assert!(
        parsed.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        parsed.diagnostics
    );
    parsed.definitions
}
