use super::{Diagnostic, DiagnosticBag, DiagnosticCode};
use crate::{SourceMap, Span};

const EXPECTED: DiagnosticCode = DiagnosticCode::new("SF1001");

#[test]
fn renders_source_excerpts_in_a_stable_form() {
    let mut sources = SourceMap::new();
    let id = sources.add("schema.graphql", "type Query {\n  name: String\n}\n");
    let diagnostic = Diagnostic::new(EXPECTED, "expected field", Span::new(id, 15, 19));

    assert_eq!(
        diagnostic.render(&sources),
        "error[SF1001]: expected field\n --> schema.graphql:2:3\n  |\n2 |   name: String\n  |   ^~~~"
    );
}

#[test]
fn diagnostic_bag_discards_entries_beyond_the_limit() {
    let mut bag = DiagnosticBag::new(2);

    assert!(bag.push(Diagnostic::without_span(EXPECTED, "first")));
    assert!(bag.push(Diagnostic::without_span(EXPECTED, "second")));
    assert!(!bag.push(Diagnostic::without_span(EXPECTED, "third")));
    assert_eq!(bag.len(), 2);
    assert!(bag.is_full());
    assert!(bag.was_truncated());
}

#[test]
fn phase_stops_when_the_diagnostic_limit_is_reached() {
    let mut bag = DiagnosticBag::new(1);

    assert!(!bag.should_stop());
    assert!(bag.push(Diagnostic::without_span(EXPECTED, "first")));
    assert!(bag.should_stop());
    assert!(!bag.was_truncated());
}

#[test]
fn zero_diagnostic_limit_stops_after_observing_the_first_error() {
    let mut bag = DiagnosticBag::new(0);

    assert!(!bag.should_stop());
    assert!(!bag.push(Diagnostic::without_span(EXPECTED, "discarded")));
    assert!(bag.should_stop());
    assert!(bag.was_truncated());
}

#[test]
fn renders_diagnostics_without_source_spans() {
    let sources = SourceMap::new();
    let mut bag = DiagnosticBag::new(1);
    assert!(bag.push(Diagnostic::without_span(EXPECTED, "standalone")));

    assert_eq!(bag.render(&sources), "error[SF1001]: standalone");
}

#[test]
fn source_excerpt_tabs_preserve_caret_position() {
    let mut sources = SourceMap::new();
    let id = sources.add("tab.graphql", "\tname");
    let diagnostic = Diagnostic::new(EXPECTED, "tab", Span::new(id, 1, 5));

    assert!(
        diagnostic
            .render(&sources)
            .ends_with("1 | \tname\n  | \t^~~~")
    );
}

#[test]
fn distinguishes_paths_and_unicode_locations_across_multiple_sources() {
    let mut sources = SourceMap::new();
    sources.add("first.graphql", "type First\n");
    let second = sources.add("second.graphql", "α\r\n\tfield");
    let diagnostic = Diagnostic::new(EXPECTED, "second source", Span::new(second, 5, 10));
    let rendered = diagnostic.render(&sources);

    assert!(rendered.contains("second.graphql:2:2"));
    assert!(rendered.ends_with("2 | \tfield\n  | \t^~~~~"));
}

#[test]
fn renders_invalid_spans_without_panicking() {
    let mut sources = SourceMap::new();
    let id = sources.add("invalid.graphql", "€");

    for span in [
        Span::new(id, 0, usize::MAX),
        Span::new(id, 3, 2),
        Span::new(id, 0, 1),
    ] {
        let rendered = Diagnostic::new(EXPECTED, "invalid", span).render(&sources);
        assert!(rendered.ends_with("invalid.graphql:<invalid span>"));
    }
    let eof = Diagnostic::new(EXPECTED, "eof", Span::new(id, 3, 3)).render(&sources);
    assert!(eof.contains("invalid.graphql:1:2"));
}

#[test]
fn rejects_invalid_diagnostic_code_formats() {
    for invalid in ["NOT_STABLE", "SF0001", "SF7001", "SF100", "ＳＦ1001"] {
        assert!(
            std::panic::catch_unwind(|| DiagnosticCode::new(invalid)).is_err(),
            "{invalid}"
        );
    }
}
