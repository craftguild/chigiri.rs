use super::{PARSE_EXECUTABLE_DEFINITION, PARSE_TYPE_SYSTEM_DEFINITION};
use super::{parse_document, parse_executable_document, parse_type_system_document};
use crate::language::ast::Definition;
use crate::{ExecutionLimits, SourceId};

const MIXED: &str = r#"
    scalar Date
    query Today { today }
    extend scalar Date @specifiedBy(url: "https://example.com/date")
    fragment DateFields on Event { date }
"#;

#[test]
fn generic_parser_preserves_both_document_domains_in_source_order() {
    let output = parse_document(SourceId::new(5), MIXED, &ExecutionLimits::unbounded());
    let document = output
        .document
        .expect("Mixed documents can be multicultured.");

    assert!(matches!(document.definitions[0], Definition::TypeSystem(_)));
    assert!(matches!(document.definitions[1], Definition::Executable(_)));
    assert!(matches!(document.definitions[2], Definition::TypeSystem(_)));
    assert!(matches!(document.definitions[3], Definition::Executable(_)));
}

#[test]
fn domain_specific_parsers_reject_the_opposite_domain_with_stable_codes() {
    let limits = ExecutionLimits::unbounded();
    let executable = parse_executable_document(SourceId::new(5), MIXED, &limits);
    let type_system = parse_type_system_document(SourceId::new(5), MIXED, &limits);

    assert!(executable.document.is_none());
    assert_eq!(
        executable.diagnostics.iter().next().unwrap().code,
        PARSE_TYPE_SYSTEM_DEFINITION
    );
    assert!(type_system.document.is_none());
    assert_eq!(
        type_system.diagnostics.iter().next().unwrap().code,
        PARSE_EXECUTABLE_DEFINITION
    );
}

#[test]
fn type_system_parser_returns_only_sdl_definitions() {
    let output = parse_type_system_document(
        SourceId::new(5),
        "scalar Date type Query { today: Date }",
        &ExecutionLimits::unbounded(),
    );
    let document = output.document.expect("valid SDL");

    assert_eq!(document.type_system_definitions().count(), 2);
    assert_eq!(document.executable_definitions().count(), 0);
}
