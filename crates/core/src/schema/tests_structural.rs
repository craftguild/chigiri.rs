use super::{
    SCHEMA_EMPTY_MEMBERS, SCHEMA_INVALID_INPUT_TYPE, SCHEMA_INVALID_INTERFACE_TARGET,
    SCHEMA_INVALID_OUTPUT_TYPE, SCHEMA_INVALID_UNION_MEMBER, SCHEMA_RESERVED_NAME,
    SCHEMA_UNKNOWN_TYPE, compose_schema,
};
use crate::language::{ast::Document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn rejects_reserved_prefixes_for_all_user_defined_schema_elements() {
    let document = parse(
        60,
        r#"
        directive @__audit(__reason: String) on OBJECT
        scalar __Scalar
        interface Named { __label(__locale: String): String }
        type Query implements Named {
          __label(__locale: String): String
          ok: Boolean
        }
        extend type Query { __later: ID }
        input __Filter { __term: String }
        enum __State { __READY }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let reserved = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_RESERVED_NAME)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert_eq!(reserved.len(), 12);
    assert!(reserved.iter().all(|diagnostic| diagnostic.span.is_some()));
}

#[test]
fn builtins_and_single_underscore_names_are_not_reserved() {
    let document = parse(
        61,
        concat!(
            "directive @_audit on OBJECT ",
            "enum State { _READY } ",
            "type Query { _field(_arg: String): State }",
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn rejects_every_memberless_named_composite_type() {
    let document = parse(
        62,
        "type Query interface EmptyInterface union EmptyUnion enum EmptyEnum input EmptyInput",
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_EMPTY_MEMBERS)
            .count(),
        5
    );
}

#[test]
fn an_extension_can_supply_the_only_member_of_a_type() {
    let document = parse(
        63,
        r#"
        interface Node
        type Query
        type User { id: ID }
        union Search
        enum State
        input Filter
        extend interface Node { id: ID }
        extend type Query { node: Node }
        extend union Search = User
        extend enum State { READY }
        extend input Filter { term: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn validates_nested_wrapper_base_types_in_input_and_output_positions() {
    let document = parse(
        64,
        r#"
        directive @invalid(owner: User, missing: [MissingDirective!]!) on OBJECT
        interface Node { id: ID }
        type User implements Node { id: ID }
        union Result = User
        input Filter { owner: User, node: Node, result: Result }
        type Query implements MissingInterface {
          invalid: [Filter!]!
          missing: [MissingOutput!]!
          search(filter: [Filter!]!, owner: User): [User!]!
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&SCHEMA_INVALID_OUTPUT_TYPE));
    assert!(
        codes
            .iter()
            .filter(|code| **code == SCHEMA_INVALID_INPUT_TYPE)
            .count()
            >= 5
    );
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == SCHEMA_UNKNOWN_TYPE)
            .count(),
        2
    );
    assert!(codes.contains(&SCHEMA_INVALID_INTERFACE_TARGET));
    assert!(output.diagnostics.iter().all(|diagnostic| {
        if matches!(
            diagnostic.code,
            SCHEMA_INVALID_INPUT_TYPE | SCHEMA_INVALID_OUTPUT_TYPE | SCHEMA_UNKNOWN_TYPE
        ) {
            diagnostic.span.is_some()
        } else {
            true
        }
    }));
}

#[test]
fn classifies_scalar_enum_and_input_types_by_input_and_output_capability() {
    let document = parse(
        65,
        r#"
        scalar Date
        enum State { READY }
        input Filter { date: Date, state: State }
        interface Node { id: ID }
        type User implements Node { id: ID }
        union Result = User
        type Query {
          scalar(filter: Filter): Date
          enum: State
          object: User
          interface: Node
          union: Result
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn union_members_must_be_unique_defined_objects_after_composition() {
    let document = parse(
        66,
        r#"
        scalar Token
        interface Node { id: ID }
        type Query { id: ID }
        union Invalid = Token | Node | Missing | Query | Query
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_UNION_MEMBER)
            .count(),
        3
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == super::SCHEMA_DUPLICATE_MEMBER)
            .count(),
        1
    );
    assert!(output.schema.is_none());
}

#[test]
fn union_members_preserve_composed_sources() {
    let document = parse(
        67,
        r#"
        type Query { id: ID }
        type User { id: ID }
        union Result = Query
        extend union Result = User
        "#,
    );
    let schema = compose_schema(&[document], &ExecutionLimits::unbounded())
        .schema
        .unwrap();
    let members = schema.type_by_name("Result").unwrap().union_members();

    assert_eq!(
        members
            .iter()
            .map(|member| member.value.as_str())
            .collect::<Vec<_>>(),
        ["Query", "User"]
    );
}
