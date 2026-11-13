use super::{SCHEMA_CONST_ONE_OF_VALUE, SCHEMA_CONST_SCALAR_VALUE, compose_schema};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId, Span};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn validates_custom_directive_definitions_and_builtin_application_types() {
    let source = concat!(
        "directive @policy(level: Int = \"high\") on OBJECT ",
        "scalar URL @specifiedBy(url: 1) ",
        "type Query { ok: String }",
    );
    let document = parse(170, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let specified_start = source.find("url: 1").unwrap() + 5;
    let specified = output
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.span
                == Some(Span::new(
                    SourceId::new(170),
                    specified_start,
                    specified_start + 1,
                ))
        })
        .unwrap();

    assert!(output.schema.is_none());
    assert_eq!(specified.code, SCHEMA_CONST_SCALAR_VALUE);
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_CONST_SCALAR_VALUE)
            .count(),
        2
    );
}

#[test]
fn one_of_directive_arguments_use_input_object_rules() {
    let document = parse(
        171,
        concat!(
            "input Pick @oneOf { text: String, number: Int } ",
            "directive @choose(value: Pick!) on OBJECT ",
            "type Query @choose(value: {}) { ok: String }",
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_CONST_ONE_OF_VALUE)
    );
}

#[test]
fn validates_provided_arguments_at_all_composed_type_system_locations() {
    let document = parse(
        172,
        r#"
        directive @argMark(value: String!) on ARGUMENT_DEFINITION
        directive @mark(value: String! @argMark(value: 1)) repeatable on
          SCHEMA | SCALAR | OBJECT | FIELD_DEFINITION | ARGUMENT_DEFINITION |
          INTERFACE | UNION | ENUM | ENUM_VALUE | INPUT_OBJECT | INPUT_FIELD_DEFINITION
        schema @mark(value: 1) { query: Query }
        scalar URL @mark(value: 1)
        interface Node @mark(value: 1) {
          id(format: String @mark(value: 1)): ID @mark(value: 1)
        }
        type Query @mark(value: 1) {
          id(format: String @mark(value: 1)): ID @mark(value: 1)
        }
        extend type Query @mark(value: 1) {
          ext(arg: String @mark(value: 1)): String @mark(value: 1)
        }
        union Search @mark(value: 1) = Query
        enum State @mark(value: 1) { READY @mark(value: 1) }
        input Filter @mark(value: 1) { term: String @mark(value: 1) }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let count = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_CONST_SCALAR_VALUE)
        .count();

    assert!(output.schema.is_none());
    assert_eq!(count, 17);
}

#[test]
fn const_value_validation_stops_at_the_limit_without_returning_a_partial_schema() {
    let document = parse(
        173,
        concat!(
            "type Query { ",
            "a(value: Int = \"x\"): String ",
            "b(value: Int = \"x\"): String ",
            "c(value: Int = \"x\"): String }",
        ),
    );
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 2;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics_were_truncated());
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SCHEMA_CONST_SCALAR_VALUE)
    );
}
