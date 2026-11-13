use crate::executable::{
    EXECUTABLE_ALIAS_LIMIT, EXECUTABLE_DEPTH_LIMIT, EXECUTABLE_DIRECTIVE_LIMIT,
    EXECUTABLE_FIELD_LIMIT, validate_executable_document,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{ExecutionLimits, SourceId};

fn schema() -> crate::schema::SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(30),
        "directive @mark on FIELD type Query { user: User } type User { friend: User name: String }",
        &ExecutionLimits::unbounded(),
    );
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

#[test]
fn reuses_fragment_dag_when_calculating_operation_complexity() {
    let source = "query Q { ...Twice } fragment Twice on Query { a: user { ...Name } b: user { ...Name } } fragment Name on User { name @mark }";
    let parsed =
        parse_executable_document(SourceId::new(31), source, &ExecutionLimits::unbounded());
    let output = validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    );
    let document = output.document.unwrap();
    let selected = document.select_operation(Some("Q")).unwrap();

    assert_eq!(selected.complexity.depth, 2);
    assert_eq!(selected.complexity.expanded_fields, 4);
    assert_eq!(selected.complexity.aliases, 2);
    assert_eq!(selected.complexity.directives, 2);
}

#[test]
fn depth_field_alias_and_directive_limits_use_distinct_codes() {
    let source = "query Q { a: user @mark { b: friend @mark { name @mark } } }";
    let parsed =
        parse_executable_document(SourceId::new(32), source, &ExecutionLimits::unbounded());
    let mut limits = ExecutionLimits::unbounded();
    limits.max_operation_depth = 2;
    limits.max_expanded_fields = 2;
    limits.max_aliases = 1;
    limits.max_directives = 2;
    let output = validate_executable_document(&schema(), &parsed.document.unwrap(), &limits);
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&EXECUTABLE_DEPTH_LIMIT));
    assert!(codes.contains(&EXECUTABLE_FIELD_LIMIT));
    assert!(codes.contains(&EXECUTABLE_ALIAS_LIMIT));
    assert!(codes.contains(&EXECUTABLE_DIRECTIVE_LIMIT));
}

#[test]
fn exponentially_referenced_fragment_dag_uses_saturating_complexity() {
    let mut source = String::from("query Q { ...F0 }\n");
    const DEPTH: usize = 128;
    for index in 0..DEPTH - 1 {
        source.push_str(&format!(
            "fragment F{index} on Query {{ ...F{} ...F{} }}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!(
        "fragment F{} on Query {{ __typename }}",
        DEPTH - 1
    ));
    let parsed =
        parse_executable_document(SourceId::new(33), &source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let output = validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    );

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
