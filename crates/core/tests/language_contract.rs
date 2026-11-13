use chigiri_core::language::ast::Definition;
use chigiri_core::language::{
    PARSE_EXECUTABLE_DEFINITION, PARSE_TYPE_SYSTEM_DEFINITION, parse_document,
    parse_executable_document, parse_type_system_document,
};
use chigiri_core::{ExecutionLimits, SourceId, SourceMap};

#[test]
fn parser_entry_points_are_publicly_accessible() {
    let limits = ExecutionLimits::unbounded();
    let source = "scalar Date query Today { today }";
    let mixed = parse_document(SourceId::new(1), source, &limits)
        .document
        .expect("mixed document");

    assert!(matches!(mixed.definitions[0], Definition::TypeSystem(_)));
    assert!(matches!(mixed.definitions[1], Definition::Executable(_)));

    let executable = parse_executable_document(SourceId::new(1), source, &limits);
    assert_eq!(
        executable.diagnostics.iter().next().unwrap().code,
        PARSE_TYPE_SYSTEM_DEFINITION
    );
    let type_system = parse_type_system_document(SourceId::new(1), source, &limits);
    assert_eq!(
        type_system.diagnostics.iter().next().unwrap().code,
        PARSE_EXECUTABLE_DEFINITION
    );
}

#[test]
fn parser_entry_points_enforce_configured_limits() {
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 2;
    limits.max_diagnostics = 1;
    let output =
        parse_executable_document(SourceId::new(2), "{ first { second { third } } }", &limits);

    assert!(output.document.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn renders_parser_diagnostics_with_unicode_locations_across_sources() {
    let mut sources = SourceMap::new();
    sources.add("preceding.graphql", "scalar Date");
    let source_id = sources.add("unicode.graphql", "query Q {\n  €\n}");
    let output = parse_executable_document(
        source_id,
        sources.get(source_id).unwrap().text(),
        &ExecutionLimits::unbounded(),
    );
    let rendered = output.diagnostics.iter().next().unwrap().render(&sources);

    assert!(rendered.contains("unicode.graphql:2:3"));
    assert!(rendered.contains("  €"));
    assert!(rendered.contains("  ^"));
}
