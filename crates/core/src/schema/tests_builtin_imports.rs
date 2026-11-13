//! Acceptance coverage for ADR-0007 SDL-01 through SDL-05 built-in imports.

use super::{
    SCHEMA_DIRECTIVE_LOCATION_MISSING, SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION,
    SCHEMA_DUPLICATE_MEMBER, SchemaOrigin, compose_schema,
};
use crate::language::ast::{Document, TypeSystemDefinition};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId, Span};

const QUERY: &str = "type Query { ok: Boolean }";

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

fn normative_directives() -> &'static str {
    r#"
    directive @skip(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    directive @deprecated(reason: String! = "No longer supported")
      on FIELD_DEFINITION | ARGUMENT_DEFINITION | INPUT_FIELD_DEFINITION | ENUM_VALUE
    directive @specifiedBy(url: String!) on SCALAR
    directive @oneOf on INPUT_OBJECT
    "#
}

fn directive_name_span(document: &Document, name: &str) -> Span {
    document
        .type_system_definitions()
        .find_map(|definition| match definition {
            TypeSystemDefinition::Directive(directive) if directive.name.value == name => {
                Some(directive.name.span)
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn sdl_01_5_identical_builtin_definitions_are_imported() {
    let document = parse(200, &format!("{}\n{QUERY}", normative_directives()));
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let schema = output.schema.unwrap();

    let builtins = schema
        .directive_definitions()
        .iter()
        .filter(|directive| directive.origin() == SchemaOrigin::BuiltIn)
        .map(|directive| directive.name())
        .collect::<Vec<_>>();
    assert_eq!(
        builtins,
        ["skip", "include", "deprecated", "specifiedBy", "oneOf"]
    );
}

#[test]
fn sdl_02_accepts_argument_and_location_reordering_only() {
    let document = parse(
        201,
        &format!(
            r#"
            directive @skip(if: Boolean!) on INLINE_FRAGMENT | FIELD | FRAGMENT_SPREAD
            directive @deprecated(reason: String! = "No longer supported")
              on ENUM_VALUE | INPUT_FIELD_DEFINITION | ARGUMENT_DEFINITION | FIELD_DEFINITION
            {QUERY}
            "#
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn ignores_sdl_03_description_string_style_differences() {
    let document = parse(
        202,
        &format!(
            r#"
            """built-in assertion"""
            directive @deprecated(
              """The client's reasoning."""
              reason: String! = """No longer supported"""
            ) on FIELD_DEFINITION | ARGUMENT_DEFINITION | INPUT_FIELD_DEFINITION | ENUM_VALUE
            {QUERY}
            "#
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn identical_builtin_imports_from_multiple_sources_are_idempotent() {
    let first = parse(203, normative_directives());
    let second = parse(204, normative_directives());
    let query = parse(205, QUERY);
    let output = compose_schema(&[first, second, query], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    assert_eq!(output.schema.unwrap().directive_definitions().len(), 5);
}

#[test]
fn sdl_04_rejects_nullable_skip_if_with_name_span_sf2004() {
    let document = parse(
        206,
        "directive @skip(if: Boolean) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT\n\
         type Query { ok: Boolean }",
    );
    let expected_span = directive_name_span(&document, "skip");
    assert_conflict(document, "skip", expected_span);
}

#[test]
fn rejects_sdl_05_deprecated_default_mismatches_with_sf2004() {
    let document = parse(
        207,
        "directive @deprecated(reason: String! = \"different\") on FIELD_DEFINITION | \
         ARGUMENT_DEFINITION | INPUT_FIELD_DEFINITION | ENUM_VALUE\n\
         type Query { ok: Boolean }",
    );
    let expected_span = directive_name_span(&document, "deprecated");
    assert_conflict(document, "deprecated", expected_span);
}

#[test]
fn sf2004_rejects_builtin_signature_location_and_repeatability_mismatches() {
    let cases = [
        (
            "include",
            "directive @include(if: Boolean!, extra: Boolean) on FIELD | \
             FRAGMENT_SPREAD | INLINE_FRAGMENT\ntype Query { ok: Boolean }",
        ),
        (
            "specifiedBy",
            "directive @specifiedBy(url: String!) on SCALAR | FIELD_DEFINITION\n\
             type Query { ok: Boolean }",
        ),
        (
            "oneOf",
            "directive @oneOf repeatable on INPUT_OBJECT\ntype Query { ok: Boolean }",
        ),
    ];

    for (offset, (name, source)) in cases.into_iter().enumerate() {
        let document = parse(211 + offset, source);
        let expected_span = directive_name_span(&document, name);
        assert_conflict(document, name, expected_span);
    }
}

#[test]
fn reports_duplicate_builtin_members_before_definition_conflicts() {
    let duplicate_argument = parse(
        208,
        "directive @skip(if: Boolean!, if: Boolean!) on FIELD | FRAGMENT_SPREAD | \
         INLINE_FRAGMENT\ntype Query { ok: Boolean }",
    );
    let duplicate_location = parse(
        209,
        "directive @oneOf on INPUT_OBJECT | INPUT_OBJECT\ntype Query { ok: Boolean }",
    );

    for document in [duplicate_argument, duplicate_location] {
        let output = compose_schema(&[document], &ExecutionLimits::unbounded());
        assert!(output.schema.is_none());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_MEMBER)
        );
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| { diagnostic.code != SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION })
        );
    }
}

#[test]
fn compares_builtin_imports_deterministically() {
    let mut document = parse(210, &format!("{}\n{QUERY}", normative_directives()));
    let directive = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            crate::language::ast::Definition::TypeSystem(TypeSystemDefinition::Directive(
                directive,
            )) if directive.name.value == "skip" => Some(directive),
            _ => None,
        })
        .unwrap();
    directive.locations.clear();
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_LOCATION_MISSING)
    );
}

fn assert_conflict(document: Document, name: &str, expected_span: Span) {
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION)
        .expect("expected built-in collision diagnostic");

    assert!(output.schema.is_none());
    assert_eq!(diagnostic.span, Some(expected_span));
    assert!(diagnostic.message.contains(&format!(
        "Built-in directive definition '@{name}' conflicts with GraphQL September 2025"
    )));
}
