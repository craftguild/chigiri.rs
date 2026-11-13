use super::{SCHEMA_DUPLICATE_TYPE, compose_schema};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn duplicate_document() -> crate::language::ast::Document {
    parse_type_system_document(
        SourceId::new(50),
        "type Query { ok: Boolean } scalar ID scalar String scalar Boolean scalar Float",
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap()
}

#[test]
fn composition_stops_when_the_diagnostic_limit_is_reached() {
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 2;
    let output = compose_schema(&[duplicate_document()], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 2);
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_TYPE)
    );
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn invalid_schema_is_not_returned_when_the_diagnostic_limit_is_zero() {
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 0;
    let output = compose_schema(&[duplicate_document()], &limits);

    assert!(output.schema.is_none());
    assert!(output.diagnostics.is_empty());
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn structural_validation_stops_at_the_diagnostic_limit() {
    let document = parse_type_system_document(
        SourceId::new(51),
        "type Query { __a: String, __b: String, __c: String }",
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap();
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 2;
    let output = compose_schema(&[document], &limits);

    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics_were_truncated());
    assert!(output.schema.is_none());
}

#[test]
fn zero_diagnostic_limit_still_allows_valid_schemas() {
    let document = parse_type_system_document(
        SourceId::new(52),
        "type Query { value: String }",
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap();
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 0;
    let output = compose_schema(&[document], &limits);

    assert!(output.is_valid());
    assert!(!output.diagnostics_were_truncated());
}
