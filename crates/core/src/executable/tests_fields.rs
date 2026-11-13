use super::directive_rules::{
    DIRECTIVE_ARGUMENT_DUPLICATE, DIRECTIVE_ARGUMENT_REQUIRED, DIRECTIVE_ARGUMENT_UNKNOWN,
    DIRECTIVE_DUPLICATE, DIRECTIVE_LOCATION_INVALID, DIRECTIVE_UNKNOWN,
};
use super::{
    FIELD_ARGUMENT_DUPLICATE, FIELD_ARGUMENT_REQUIRED, FIELD_ARGUMENT_UNKNOWN, FIELD_NOT_DEFINED,
    FIELD_SELECTION_REQUIRED, LEAF_SELECTION_PRESENT, ROOT_TYPE_MISSING, field_merge,
    validate_fields,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits, SourceId};

fn schema(source: &str) -> crate::schema::SemanticSchema {
    let parsed =
        parse_type_system_document(SourceId::new(30), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

fn validate(schema: &crate::schema::SemanticSchema, source: &str) -> DiagnosticBag {
    let parsed =
        parse_executable_document(SourceId::new(31), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let mut diagnostics = DiagnosticBag::new(100);
    validate_fields(schema, &parsed.document.unwrap(), &mut diagnostics);
    diagnostics
}

fn codes(diagnostics: &DiagnosticBag) -> Vec<DiagnosticCode> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn test_schema() -> crate::schema::SemanticSchema {
    schema(
        r#"
        directive @fieldOnly(required: Boolean!, optional: Int) on FIELD
        type Query {
          user(id: ID!, limit: Int): User
          pair(a: Int, b: Int, filter: Filter): String
          ok: Boolean
        }
        type User { id: ID! name: String friend: User }
        input Filter { x: Int y: Int }
        "#,
    )
}

#[test]
fn distinguishes_field_shape_from_argument_structure() {
    let diagnostics = validate(
        &test_schema(),
        r#"{
          missing
          ok { value }
          user(id: "1", id: "2", extra: 1)
        }"#,
    );
    let found_codes = codes(&diagnostics);

    assert!(found_codes.contains(&FIELD_NOT_DEFINED));
    assert!(found_codes.contains(&LEAF_SELECTION_PRESENT));
    assert!(found_codes.contains(&FIELD_ARGUMENT_DUPLICATE));
    assert!(found_codes.contains(&FIELD_ARGUMENT_UNKNOWN));
    assert!(found_codes.contains(&FIELD_SELECTION_REQUIRED));

    let missing = validate(&test_schema(), "{ user { id } }");
    assert!(codes(&missing).contains(&FIELD_ARGUMENT_REQUIRED));
}

#[test]
fn validates_directive_locations_repetition_and_argument_structure() {
    let diagnostics = validate(
        &test_schema(),
        r#"query Q @fieldOnly @missing {
          ok @fieldOnly @fieldOnly(required: true, required: false, extra: 1)
        }"#,
    );
    let codes = codes(&diagnostics);

    assert!(codes.contains(&DIRECTIVE_UNKNOWN));
    assert!(codes.contains(&DIRECTIVE_LOCATION_INVALID));
    assert!(codes.contains(&DIRECTIVE_DUPLICATE));
    assert!(codes.contains(&DIRECTIVE_ARGUMENT_DUPLICATE));
    assert!(codes.contains(&DIRECTIVE_ARGUMENT_UNKNOWN));
    assert!(codes.contains(&DIRECTIVE_ARGUMENT_REQUIRED));
}

#[test]
fn required_argument_errors_preserve_schema_diagnostics() {
    let schema = schema(
        r#"
        directive @ordered(z: Int!, a: Int!) on FIELD
        type Query { run(z: Int!, a: Int!): Boolean }
        "#,
    );
    let diagnostics = validate(&schema, "{ run @ordered }");
    let messages = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code,
                FIELD_ARGUMENT_REQUIRED | DIRECTIVE_ARGUMENT_REQUIRED
            )
        })
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();

    assert_eq!(messages.len(), 4);
    assert!(messages[0].contains("`z`"));
    assert!(messages[1].contains("`a`"));
    assert!(messages[2].contains("`z`"));
    assert!(messages[3].contains("`a`"));
}

#[test]
fn diagnoses_a_missing_root_for_the_operation_type() {
    let diagnostics = validate(&test_schema(), "mutation M { ok }");

    assert_eq!(codes(&diagnostics), vec![ROOT_TYPE_MISSING]);
}

#[test]
fn tiny_query_does_not_preconstruct_an_unrelated_wide_field_catalog() {
    let mut source = "type Query { ok: Boolean } type Wide { field(".to_owned();
    for index in 0..10_000 {
        source.push_str(&format!("arg{index}: String "));
    }
    source.push_str("): Boolean }");
    let schema = schema(&source);
    let diagnostics = validate(&schema, "{ ok }");

    assert!(diagnostics.is_empty(), "{:?}", diagnostics.into_vec());
}

#[test]
fn detects_collisions_across_alias_arguments_and_nested_selections() {
    let diagnostics = validate(
        &test_schema(),
        r#"{
          same: user(id: "1") { value: id }
          same: user(id: "2") { value: name }
        }"#,
    );
    let codes = codes(&diagnostics);

    assert!(codes.contains(&field_merge::MERGE_ARGUMENTS));
    assert!(codes.contains(&field_merge::MERGE_FIELD_NAME));
}

#[test]
fn mutually_exclusive_object_fragments_do_not_conflict() {
    let schema = schema(
        r#"
        type Query { result: Result }
        union Result = User | Admin
        type User { value: String }
        type Admin { count: String }
        "#,
    );
    let diagnostics = validate(
        &schema,
        r#"{ result {
          ... on User { answer: value }
          ... on Admin { answer: count }
        } }"#,
    );

    assert!(diagnostics.is_empty(), "{:?}", diagnostics.into_vec());
}

#[test]
fn merges_equivalent_input_objects_and_arguments_in_different_orders() {
    let diagnostics = validate(
        &test_schema(),
        r#"{
          value: pair(a: 1, b: 2, filter: { x: 3, y: 4 })
          value: pair(filter: { y: 4, x: 3 }, b: 2, a: 1)
        }"#,
    );

    assert!(diagnostics.is_empty(), "{:?}", diagnostics.into_vec());
}

#[test]
fn nested_merges_preserve_diagnostics_before_later_siblings() {
    let schema = schema(
        r#"
        type Query { user: User one: String two: String }
        type User { id: ID name: String }
        "#,
    );
    let diagnostics = validate(
        &schema,
        "{ user { value: id value: name } sibling: one sibling: two }",
    );
    let spans = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == field_merge::MERGE_FIELD_NAME)
        .map(|diagnostic| diagnostic.span.unwrap().start)
        .collect::<Vec<_>>();

    assert_eq!(spans.len(), 2);
    assert!(spans[0] < spans[1]);
}

#[test]
fn fragment_cycles_do_not_revisit_selection_sets_indefinitely() {
    let diagnostics = validate(
        &test_schema(),
        r#"
        query Q { ...A }
        fragment A on Query { ok ...B }
        fragment B on Query { ok ...A }
        "#,
    );

    assert!(diagnostics.is_empty(), "{:?}", diagnostics.into_vec());
}

#[test]
fn zero_diagnostic_limit_still_marks_field_discovery_as_truncated() {
    let parsed = parse_executable_document(
        SourceId::new(32),
        "{ missing }",
        &ExecutionLimits::unbounded(),
    );
    let mut diagnostics = DiagnosticBag::new(0);
    let valid = validate_fields(&test_schema(), &parsed.document.unwrap(), &mut diagnostics);

    assert!(!valid);
    assert!(diagnostics.was_truncated());
}
