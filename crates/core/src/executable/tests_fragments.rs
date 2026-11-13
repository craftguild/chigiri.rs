use super::fragment_rules::{
    FRAGMENT_INVALID_TYPE_CONDITION, FRAGMENT_SPREAD_CYCLE, FRAGMENT_SPREAD_NOT_POSSIBLE,
    FRAGMENT_SPREAD_TARGET_UNDEFINED, FRAGMENT_UNUSED, validate_fragments,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits, SourceId};

use super::{field_rules::FIELD_NOT_DEFINED, validate_executable_document};

fn schema() -> SemanticSchema {
    let source = r#"
        interface Node { id: ID! }
        type User implements Node { id: ID! }
        type Photo { id: ID! }
        union Search = User | Photo
        type Query { node: Node search: Search user: User }
    "#;
    let parsed =
        parse_type_system_document(SourceId::new(41), source, &ExecutionLimits::unbounded());
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

fn diagnostics(source: &str, max_diagnostics: usize) -> DiagnosticBag {
    let parsed =
        parse_executable_document(SourceId::new(42), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let mut diagnostics = DiagnosticBag::new(max_diagnostics);
    let _ = validate_fragments(&schema(), &parsed.document.unwrap(), &mut diagnostics);
    diagnostics
}

fn validate(source: &str, max_diagnostics: usize) -> Vec<DiagnosticCode> {
    diagnostics(source, max_diagnostics)
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn type_condition_is_limited_to_defined_composite() {
    let codes = validate(
        "query Q { user { id } } fragment ScalarBits on String { __typename } fragment MissingBits on Missing { __typename }",
        10,
    );

    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == FRAGMENT_INVALID_TYPE_CONDITION)
            .count(),
        2
    );
}

#[test]
fn rejects_undefined_fragment_spread_targets() {
    let codes = validate("query Q { user { ...Missing } }", 10);

    assert_eq!(codes, vec![FRAGMENT_SPREAD_TARGET_UNDEFINED]);
}

#[test]
fn diagnostics_record_the_validation_phase() {
    let parsed = parse_executable_document(
        SourceId::new(43),
        "query Q { missing ...Missing }",
        &ExecutionLimits::unbounded(),
    );
    let output = validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    );
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert_eq!(
        codes,
        vec![FRAGMENT_SPREAD_TARGET_UNDEFINED, FIELD_NOT_DEFINED]
    );
}

#[test]
fn fragment_diagnostics_record_the_rule_subphase_sequence() {
    let codes = validate(
        "fragment A on User { ...A } query Q { user { ...Missing ...A } }",
        10,
    );

    assert_eq!(
        codes,
        vec![FRAGMENT_SPREAD_TARGET_UNDEFINED, FRAGMENT_SPREAD_CYCLE]
    );
}

#[test]
fn detects_fragment_cycles_with_iterative_depth_first_search() {
    let codes = validate(
        "query Q { user { ...A } } fragment A on User { ...B } fragment B on User { ...A }",
        10,
    );

    assert!(codes.contains(&FRAGMENT_SPREAD_CYCLE));
    assert!(!codes.contains(&FRAGMENT_UNUSED));
}

#[test]
fn detects_a_ten_thousand_fragment_cycle_without_stack_recursion() {
    let count = 10_000;
    let mut source = "query Q { user { ...F0 } } ".to_owned();
    for index in 0..count {
        let target = (index + 1) % count;
        source.push_str(&format!("fragment F{index} on User {{ ...F{target} }} "));
    }
    let codes = validate(&source, 10);

    assert!(codes.contains(&FRAGMENT_SPREAD_CYCLE));
    assert!(!codes.contains(&FRAGMENT_UNUSED));
}

#[test]
fn rejects_fragment_cycles_reachable_from_an_operation() {
    let codes = validate(
        "query Q { user { ...Used } } fragment Used on User { id } fragment DeadA on User { ...DeadB } fragment DeadB on User { id }",
        10,
    );

    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == FRAGMENT_UNUSED)
            .count(),
        2
    );
}

#[test]
fn fragment_spread_validity_uses_intersecting_possible_object_sets() {
    let valid = validate(
        "query Q { node { ...SearchBits } search { ... on Node { id } } } fragment SearchBits on Search { __typename }",
        10,
    );
    assert!(valid.is_empty());

    let invalid = validate(
        "query Q { node { ...PhotoBits } } fragment PhotoBits on Photo { id }",
        10,
    );
    assert_eq!(invalid, vec![FRAGMENT_SPREAD_NOT_POSSIBLE]);
}

#[test]
fn introspection_objects_are_valid_conditions_for_named_and_inline_fragments() {
    let codes = validate(
        r#"query Q {
          __type(name: "Query") {
            ...FullType
            ... on __Type { name }
          }
        }
        fragment FullType on __Type { kind }
        "#,
        10,
    );

    assert!(codes.is_empty());
}

#[test]
fn september_2025_standard_introspection_query_can_be_documented_and_verified() {
    let source = r#"
      query IntrospectionQuery {
        __schema {
          description
          queryType { name kind }
          mutationType { name kind }
          subscriptionType { name kind }
          types { ...FullType }
          directives {
            name
            description
            isRepeatable
            locations
            args(includeDeprecated: true) { ...InputValue }
          }
        }
      }
      fragment FullType on __Type {
        kind
        name
        description
        specifiedByURL
        fields(includeDeprecated: true) {
          name
          description
          args(includeDeprecated: true) { ...InputValue }
          type { ...TypeRef }
          isDeprecated
          deprecationReason
        }
        inputFields(includeDeprecated: true) { ...InputValue }
        interfaces { ...TypeRef }
        enumValues(includeDeprecated: true) {
          name
          description
          isDeprecated
          deprecationReason
        }
        possibleTypes { ...TypeRef }
        isOneOf
      }
      fragment InputValue on __InputValue {
        name
        description
        type { ...TypeRef }
        defaultValue
        isDeprecated
        deprecationReason
      }
      fragment TypeRef on __Type {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType { kind name }
          }
        }
      }
    "#;
    let parsed =
        parse_executable_document(SourceId::new(43), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());

    let output = super::validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    );
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn fragment_validation_stops_at_the_diagnostic_limit() {
    let output = diagnostics(
        "query Q { user { ...MissingA ...MissingB } } fragment Dead on User { id }",
        1,
    );
    let codes = output
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert_eq!(codes, vec![FRAGMENT_SPREAD_TARGET_UNDEFINED]);
    assert!(output.iter().next().unwrap().message.contains("MissingA"));
    assert!(output.is_full());
    assert!(!output.was_truncated());

    let zero = diagnostics("query Q { user { ...Missing } }", 0);
    assert!(zero.is_empty());
    assert!(zero.was_truncated());
}
