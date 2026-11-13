use super::super::{PARSE_NESTING_LIMIT, PARSE_UNEXPECTED_TOKEN};
use super::tests_support::{parse, parse_with_limits};
use crate::{ExecutionLimits, SourceId, Span};

#[test]
fn september_2025_rejects_member_blocks() {
    for source in [
        "schema {}",
        "type Empty {}",
        "interface Empty {}",
        "enum Empty {}",
        "input Empty {}",
    ] {
        let parsed = parse(source);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN),
            "source={source:?}, diagnostics={:?}",
            parsed.diagnostics
        );
        let close = source.find('}').unwrap();
        assert_eq!(
            parsed.diagnostics[0].span,
            Some(Span::new(SourceId::new(42), close, close + 1))
        );
    }
}

#[test]
fn rejects_empty_argument_definitions_and_reserved_enum_values() {
    for source in ["type Query { field(): Int }", "directive @auth() on FIELD"] {
        let parsed = parse(source);
        assert!(!parsed.diagnostics.is_empty(), "source={source:?}");
    }

    let parsed = parse("enum Truth { true false null }");
    assert_eq!(parsed.diagnostics.len(), 3);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN)
    );
}

#[test]
fn extensions_require_at_least_one_additional_element() {
    for source in [
        "extend schema",
        "extend scalar Date",
        "extend type Product",
        "extend interface Resource",
        "extend union Search",
        "extend enum Direction",
        "extend input Filter",
        "extend type Product {}",
        "extend interface Resource {}",
        "extend enum Direction {}",
        "extend input Filter {}",
    ] {
        let parsed = parse(source);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN),
            "source={source:?}, diagnostics={:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn rejects_incomplete_implements_union_schema_and_directive_locations() {
    for source in [
        "type Product implements",
        "union Search =",
        "union Search = |",
        "schema { root: Query }",
        "directive @auth on",
        "directive @auth on |",
        "directive @auth on UNKNOWN",
    ] {
        let parsed = parse(source);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN),
            "source={source:?}, diagnostics={:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn type_system_const_positions_reject_nested_variables_at_their_spans() {
    for source in [
        "input Filter { value: Int = $x }",
        "input Filter { value: [Int] = [$x] }",
        "input Filter { value: Config = { nested: $x } }",
        "scalar Date @tag(value: { nested: [$x] })",
    ] {
        let parsed = parse(source);
        let variable = source.find("$x").unwrap();
        assert!(
            parsed.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == PARSE_UNEXPECTED_TOKEN
                    && diagnostic.span == Some(Span::new(SourceId::new(42), variable, variable + 2))
            }),
            "source={source:?}, diagnostics={:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn rejects_double_non_null_and_descriptions_before_extensions_while_preserving_origins() {
    let source = "type Query { field: String!! }";
    let parsed = parse(source);
    let second_bang = source.rfind('!').unwrap();
    assert_eq!(
        parsed.diagnostics[0].span,
        Some(Span::new(SourceId::new(42), second_bang, second_bang + 1))
    );

    let source = "\"not allowed\" extend type Query @tag";
    let parsed = parse(source);
    assert_eq!(
        parsed.diagnostics[0].span,
        Some(Span::new(SourceId::new(42), 0, 13))
    );
}

#[test]
fn deeply_nested_sdl_defaults_stop_at_the_configured_limit() {
    let source = "input Deep { value: [[[[Int]]]] = [[[[1]]]] }";
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 3;
    let parsed = parse_with_limits(source, &limits);

    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PARSE_NESTING_LIMIT)
    );
    assert!(!parsed.at_eof);
}
