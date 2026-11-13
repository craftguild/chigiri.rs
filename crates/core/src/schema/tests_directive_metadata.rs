use super::{SCHEMA_DEPRECATED_REQUIRED_INPUT, SCHEMA_INVALID_SPECIFIED_BY_TARGET, compose_schema};
use crate::language::{ast::Document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn specified_by_is_allowed_only_on_custom_scalars() {
    let document = parse(
        80,
        r#"
        scalar Date @specifiedBy(url: "https://example.com/date")
        scalar Token
        extend scalar Token @specifiedBy(url: "https://example.com/token")
        extend scalar String @specifiedBy(url: "https://example.com/string")
        input Filter @specifiedBy(url: "https://example.com/filter") { term: String }
        type Query @specifiedBy(url: "https://example.com/query") { date: Date }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_SPECIFIED_BY_TARGET)
            .count(),
        3
    );
    assert!(output.schema.is_none());
}

#[test]
fn rejects_deprecation_on_required_inputs_without_defaults_across_argument_kinds() {
    let document = parse(
        81,
        r#"
        directive @audit(required: String! @deprecated) on FIELD
        input Filter {
          required: String! @deprecated
          nullable: String @deprecated
          defaulted: String! = "x" @deprecated
        }
        type Query {
          find(
            required: ID! @deprecated
            nullable: ID @deprecated
            defaulted: ID! = "x" @deprecated
          ): String
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_DEPRECATED_REQUIRED_INPUT)
            .count(),
        3
    );
    assert!(output.schema.is_none());
}

#[test]
fn deprecated_input_values_may_be_nullable_or_have_defaults() {
    let document = parse(
        82,
        r#"
        input Filter {
          nullable: String @deprecated
          defaulted: String! = "x" @deprecated
        }
        type Query {
          find(nullable: ID @deprecated, defaulted: ID! = "x" @deprecated): String
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
