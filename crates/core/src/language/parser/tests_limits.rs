use super::parse_executable_document;
use super::{PARSE_DOCUMENT_TOO_LARGE, PARSE_NESTING_LIMIT, PARSE_UNEXPECTED_TOKEN};
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str, limits: &ExecutionLimits) -> super::ParseOutput {
    parse_executable_document(SourceId::new(11), source, limits)
}

#[test]
fn applies_the_document_byte_limit_before_lexing() {
    let mut limits = ExecutionLimits::production();
    limits.max_document_bytes = 4;
    let output = parse("{ abc }", &limits);

    assert!(output.document.is_none());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        PARSE_DOCUMENT_TOO_LARGE
    );
}

#[test]
fn document_byte_limit_uses_bytes_for_ascii_and_multibyte_boundaries() {
    for source in ["{ field }", "{ field(value: \"Name\") }"] {
        let mut limits = ExecutionLimits::unbounded();
        limits.max_document_bytes = source.len();
        assert!(parse(source, &limits).is_valid(), "source: {source}");

        limits.max_document_bytes -= 1;
        let output = parse(source, &limits);
        assert!(output.document.is_none());
        assert_eq!(
            output.diagnostics.iter().next().unwrap().code,
            PARSE_DOCUMENT_TOO_LARGE
        );
    }
}

#[test]
fn parser_nesting_limits_are_inclusive_for_each_recursive_syntax() {
    let cases = [
        ("{ first { second } }", 2),
        ("{ field(value: [1]) }", 2),
        ("query Q($value: [[Int]]) { field }", 2),
        ("type Query { field: String }", 1),
    ];
    for (source, required_nesting) in cases {
        let mut limits = ExecutionLimits::unbounded();
        limits.max_parser_nesting = required_nesting;
        let accepted = if source.starts_with("type") {
            crate::language::parse_type_system_document(SourceId::new(11), source, &limits)
        } else {
            parse(source, &limits)
        };
        assert!(
            accepted.is_valid(),
            "the configured limit should be inclusive"
        );

        limits.max_parser_nesting = required_nesting - 1;
        let rejected = if source.starts_with("type") {
            crate::language::parse_type_system_document(SourceId::new(11), source, &limits)
        } else {
            parse(source, &limits)
        };
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == PARSE_NESTING_LIMIT),
            "the input should exceed the configured maximum"
        );
    }
}

#[test]
fn rejects_excessive_selection_nesting_without_stack_recursion() {
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 3;
    let output = parse("{ a { b { c { d } } } }", &limits);

    assert!(output.document.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PARSE_NESTING_LIMIT)
    );
}

#[test]
fn applies_the_same_nesting_limit_to_values_and_type_references() {
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 3;
    for source in [
        "{ field(value: [[[[1]]]]) }",
        "query Q($value: [[[[Int]]]]) { field }",
        "{ field(value: {a: {b: {c: {d: 1}}}}) }",
    ] {
        let output = parse(source, &limits);
        assert!(output.document.is_none(), "source: {source}");
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == PARSE_NESTING_LIMIT)
        );
    }
}

#[test]
fn very_deep_input_is_rejected_without_stack_overflow() {
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 8;
    let source = format!(
        "{{ field(value: {}1{}) }}",
        "[".repeat(10_000),
        "]".repeat(10_000)
    );

    let output = parse(&source, &limits);
    assert!(output.document.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|item| item.code == PARSE_NESTING_LIMIT)
    );
}

#[test]
fn diagnostic_limit_stops_parser_recovery() {
    let mut limits = ExecutionLimits::production();
    limits.max_diagnostics = 2;
    let output = parse("{ ! ! ! ! ! ! }", &limits);

    assert!(output.document.is_none());
    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics_were_truncated());
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == PARSE_UNEXPECTED_TOKEN)
    );
}

#[test]
fn zero_diagnostic_limit_preserves_error_and_truncation_status() {
    let mut limits = ExecutionLimits::production();
    limits.max_diagnostics = 0;
    let output = parse("{ ! }", &limits);

    assert!(output.document.is_none());
    assert!(output.diagnostics.is_empty());
    assert!(output.diagnostics_were_truncated());
}
