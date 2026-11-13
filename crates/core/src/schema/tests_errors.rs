use super::{
    SCHEMA_DUPLICATE_DEFINITION, SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION,
    SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION, SCHEMA_DUPLICATE_MEMBER, SCHEMA_DUPLICATE_ROOT,
    SCHEMA_DUPLICATE_TYPE, SCHEMA_EXECUTABLE_DEFINITION, SCHEMA_EXTENSION_KIND_MISMATCH,
    SCHEMA_EXTENSION_TARGET_MISSING, SCHEMA_QUERY_ROOT_MISSING, SCHEMA_ROOT_TYPE_MISSING,
    SCHEMA_ROOT_TYPE_NOT_OBJECT, SCHEMA_ROOT_TYPES_NOT_DISTINCT, compose_schema,
};
use crate::language::{ast::Document, parse_document, parse_type_system_document};
use crate::{DiagnosticCode, ExecutionLimits, SourceId};

fn parse_sdl(id: usize, source: &str) -> Document {
    parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded())
        .document
        .unwrap()
}

fn codes(output: &super::SchemaBuildOutput) -> Vec<DiagnosticCode> {
    output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn duplicate_definitions_members_and_directives_use_stable_codes() {
    let document = parse_sdl(
        40,
        r#"
        directive @flag on OBJECT
        directive @flag on FIELD_DEFINITION
        type Query @deprecated @deprecated {
          same(arg: Int, arg: Int): String
          same: ID
        }
        scalar Int
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    let actual = codes(&output);
    assert!(actual.contains(&SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION));
    assert!(actual.contains(&SCHEMA_DUPLICATE_MEMBER));
    assert!(actual.contains(&SCHEMA_DUPLICATE_TYPE));
    assert!(actual.contains(&SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION));
}

#[test]
fn reports_the_missing_extension_member_kind() {
    let document = parse_sdl(
        41,
        r#"
        type Query { id: ID }
        scalar Token
        extend type Missing { value: String }
        extend type Token { value: String }
        extend type Query { id: ID }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let actual = codes(&output);

    assert!(actual.contains(&SCHEMA_EXTENSION_TARGET_MISSING));
    assert!(actual.contains(&SCHEMA_EXTENSION_KIND_MISMATCH));
    assert!(actual.contains(&SCHEMA_DUPLICATE_MEMBER));
}

#[test]
fn reports_all_root_composition_errors() {
    let document = parse_sdl(
        42,
        r#"
        schema {
          query: Missing
          query: Missing
          mutation: ScalarRoot
          subscription: Missing
        }
        scalar ScalarRoot
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let actual = codes(&output);

    assert!(actual.contains(&SCHEMA_DUPLICATE_ROOT));
    assert!(actual.contains(&SCHEMA_ROOT_TYPE_MISSING));
    assert!(actual.contains(&SCHEMA_ROOT_TYPE_NOT_OBJECT));
    assert!(actual.contains(&SCHEMA_ROOT_TYPES_NOT_DISTINCT));
}

#[test]
fn accepts_a_subscription_root_object() {
    let document = parse_sdl(
        48,
        r#"
        schema { query: Query subscription: Subscription }
        type Query { ok: Boolean }
        type Subscription { event: Boolean }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let schema = output.schema.expect("subscription roots are supported");

    assert!(output.diagnostics.is_empty());
    assert_eq!(
        schema.roots().subscription().unwrap().type_name,
        "Subscription"
    );
}

#[test]
fn missing_query_roots_and_mixed_executable_definitions_prevent_schema_output() {
    let no_query = compose_schema(
        &[parse_sdl(43, "scalar Date")],
        &ExecutionLimits::unbounded(),
    );
    assert_eq!(codes(&no_query), [SCHEMA_QUERY_ROOT_MISSING]);
    assert!(no_query.schema.is_none());

    let mixed = parse_document(
        SourceId::new(44),
        "type Query { ok: Boolean } query Run { ok }",
        &ExecutionLimits::unbounded(),
    )
    .document
    .unwrap();
    let output = compose_schema(&[mixed], &ExecutionLimits::unbounded());
    assert!(codes(&output).contains(&SCHEMA_EXECUTABLE_DEFINITION));
    assert!(output.schema.is_none());
}

#[test]
fn validates_schema_definitions_and_extension_targets() {
    let duplicates = parse_sdl(
        45,
        "schema { query: Query } schema { query: Query } type Query { ok: Boolean }",
    );
    let output = compose_schema(&[duplicates], &ExecutionLimits::unbounded());
    assert!(codes(&output).contains(&SCHEMA_DUPLICATE_DEFINITION));

    let missing = parse_sdl(
        46,
        "extend schema { query: Root } type Root { ok: Boolean }",
    );
    let output = compose_schema(&[missing], &ExecutionLimits::unbounded());
    assert!(codes(&output).contains(&SCHEMA_EXTENSION_TARGET_MISSING));
}

#[test]
fn duplicate_members_across_all_type_kinds_use_the_same_stable_code() {
    let document = parse_sdl(
        47,
        r#"
        interface Node { id: ID }
        interface Resource implements Node & Node { id: ID }
        type Query implements Node & Node { id: ID }
        union Result = Query | Query
        enum State { READY READY }
        input Filter { term: String term: String }
        extend interface Node { id: ID }
        extend union Result = Query
        extend enum State { READY }
        extend input Filter { term: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert!(
        codes(&output)
            .iter()
            .filter(|code| **code == SCHEMA_DUPLICATE_MEMBER)
            .count()
            >= 8
    );
}
