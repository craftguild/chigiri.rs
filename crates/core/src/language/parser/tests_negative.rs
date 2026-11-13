use super::{PARSE_TYPE_SYSTEM_DEFINITION, PARSE_UNEXPECTED_TOKEN};
use super::{ParseOutput, parse_executable_document};
use crate::language::LEX_INVALID_NUMBER;
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str) -> ParseOutput {
    parse_executable_document(SourceId::new(4), source, &ExecutionLimits::unbounded())
}

fn assert_syntax_error(source: &str) {
    let output = parse(source);
    assert!(output.document.is_none(), "invalid source: {source}");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN),
        "expected a diagnostic"
    );
}

#[test]
fn rejects_empty_documents_selections_and_argument_lists() {
    for source in ["", "{}", "query Q() { field }", "{ field() }"] {
        assert_syntax_error(source);
    }
}

#[test]
fn rejects_incomplete_selections_fragments_and_type_references() {
    for source in [
        "query Q { field",
        "fragment on on User { id }",
        "fragment F User { id }",
        "query Q($id: ID!!) { field }",
        "query Q($id: [ID!) { field }",
        "{ ... on { id } }",
        "{ alias: }",
    ] {
        assert_syntax_error(source);
    }
}

#[test]
fn rejects_variables_recursively_in_const_positions() {
    for source in [
        "query Q($x: Int = $other) { field }",
        "query Q($x: [Int] = [$other]) { field }",
        "query Q($x: Input = { nested: $other }) { field }",
        "query Q($x: Int @d(value: $other)) { field }",
    ] {
        assert_syntax_error(source);
    }
}

#[test]
fn executable_parser_rejects_all_type_system_definitions() {
    let sources = [
        "schema { query: Query }",
        "scalar Date",
        "type Query { field: String }",
        "interface Node { id: ID! }",
        "union Result = User",
        "enum Color { RED }",
        "input Filter { term: String }",
        "directive @tag on FIELD",
        "extend type Query { other: String }",
        "\"docs\" type Query { field: String }",
    ];
    for source in sources {
        let output = parse(source);
        assert!(output.document.is_none());
        assert_eq!(
            output.diagnostics.iter().next().unwrap().code,
            PARSE_TYPE_SYSTEM_DEFINITION,
            "source: {source}"
        );
    }
}

#[test]
fn rejects_partial_tokens_after_lexical_errors() {
    let output = parse("{ valid invalid(value: 01) }");

    assert!(output.document.is_none());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        LEX_INVALID_NUMBER
    );
}

#[test]
fn rejects_description_without_a_following_definition() {
    assert_syntax_error("\"docs\" { field }");
}

#[test]
fn parser_leaves_semantic_uniqueness_rules_to_validation() {
    let output = parse("query Q($x: Int, $x: Int) { a(v: 1, v: 2) } query Q { b }");

    assert!(output.is_valid());
}
