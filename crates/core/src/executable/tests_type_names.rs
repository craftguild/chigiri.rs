//! Regression tests for long schema names across validation work queues.

use super::field_support::{FieldArgumentCatalog, field_output};
use crate::executable::{EXECUTABLE_FIELD_LIMIT, validate_executable_document};
use crate::language::ast::{Definition, ExecutableDefinition, Selection};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, SourceId};
use std::fmt::Write as _;

fn schema(source: &str) -> SemanticSchema {
    assert!(source.len() < 128 * 1024);
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(SourceId::new(71), source, &limits);
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics);
    compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap()
}

#[test]
fn rsm_16_long_output_names_are_borrowed_through_selection_and_merge_validation() {
    let name = "Long".repeat(8192);
    let schema = schema(&format!(
        "type Query {{ node: {name} }} type {name} {{ x: Int }}"
    ));
    let source = format!("{{node{{ {} }}}}", "x ".repeat(10_000));
    let mut limits = ExecutionLimits::production();
    let parsed = parse_executable_document(SourceId::new(72), &source, &limits);
    assert!(parsed.is_valid());
    let document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let Selection::Field(field) = &operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    let first = field_output(&schema, "Query", field).unwrap();
    for _ in 0..10_000 {
        let next = field_output(&schema, "Query", field).unwrap();
        assert!(std::ptr::eq(first.named, next.named));
    }
    limits.max_expanded_fields = 10_000;
    let rejected = validate_executable_document(&schema, &document, &limits);
    assert!(!rejected.is_valid());
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.code == EXECUTABLE_FIELD_LIMIT)
    );
    limits.max_expanded_fields = 10_001;
    assert!(validate_executable_document(&schema, &document, &limits).is_valid());
}

#[test]
fn rsm_16_long_input_names_support_many_list_items_and_variable_sites() {
    let name = "Long".repeat(8192);
    let schema = schema(&format!(
        "input {name} {{ x: Int }} type Query {{ run(values: [{name}]): Int }}"
    ));
    for source in [
        format!("{{run(values:[{}])}}", "{} ".repeat(10_000)),
        format!("query($v:{name}){{run(values:[{}])}}", "$v ".repeat(10_000)),
    ] {
        let limits = ExecutionLimits::production();
        let parsed = parse_executable_document(SourceId::new(73), &source, &limits);
        assert!(parsed.is_valid());
        let result = validate_executable_document(&schema, &parsed.document.unwrap(), &limits);
        assert!(result.is_valid(), "{:?}", result.diagnostics);
    }
}

#[test]
fn rsm_16_many_field_rules_share_the_long_parent_name() {
    let name = "Long".repeat(8192);
    let mut fields = String::new();
    for i in 0..3000 {
        write!(fields, "f{i}:Int ").unwrap();
    }
    let schema = schema(&format!("schema {{query:{name}}} type {name} {{{fields}}}"));
    let mut catalog = FieldArgumentCatalog::new(&schema);
    for i in 0..3000 {
        assert!(catalog.get(&name, &format!("f{i}")).is_some());
    }
}
