use super::{
    ResolvedSchemaCoordinate, SCHEMA_COORDINATE_CONTAINER_MISSING,
    SCHEMA_COORDINATE_INVALID_CONTAINER, parse_schema_coordinate, resolve_schema_coordinate,
};
use crate::language::{ast::Document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{ExecutionLimits, SourceId};

fn parse_document(source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(31), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

fn schema() -> SemanticSchema {
    let document = parse_document(
        r#"
        directive @auth(role: String) on FIELD_DEFINITION
        enum Status { ACTIVE }
        input Filter { term: String }
        interface Node { id: ID! }
        type Query implements Node {
          id: ID!
          user(filter: Filter): Node
        }
        union Search = Query
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.schema.unwrap()
}

fn resolve<'schema>(
    schema: &'schema SemanticSchema,
    source: &str,
) -> Option<ResolvedSchemaCoordinate<'schema>> {
    let coordinate = parse_schema_coordinate(SourceId::new(42), source).unwrap();
    resolve_schema_coordinate(schema, &coordinate).unwrap()
}

#[test]
fn resolves_type_field_argument_input_field_and_enum_coordinates_exactly() {
    let schema = schema();

    assert!(
        matches!(resolve(&schema, "Query"), Some(ResolvedSchemaCoordinate::Type(value)) if value.name() == "Query")
    );
    assert!(
        matches!(resolve(&schema, "Query.user"), Some(ResolvedSchemaCoordinate::Field(value)) if value.name.value == "user")
    );
    assert!(
        matches!(resolve(&schema, "Filter.term"), Some(ResolvedSchemaCoordinate::InputField(value)) if value.name.value == "term")
    );
    assert!(
        matches!(resolve(&schema, "Status.ACTIVE"), Some(ResolvedSchemaCoordinate::EnumValue(value)) if value.name.value == "ACTIVE")
    );
    assert!(
        matches!(resolve(&schema, "Query.user(filter:)"), Some(ResolvedSchemaCoordinate::FieldArgument(value)) if value.name.value == "filter")
    );
}

#[test]
fn resolves_custom_and_builtin_directive_arguments() {
    let schema = schema();

    assert!(
        matches!(resolve(&schema, "@auth"), Some(ResolvedSchemaCoordinate::Directive(value)) if value.name() == "auth")
    );
    assert!(
        matches!(resolve(&schema, "@auth(role:)"), Some(ResolvedSchemaCoordinate::DirectiveArgument(value)) if value.name == "role")
    );
    assert!(
        matches!(resolve(&schema, "String"), Some(ResolvedSchemaCoordinate::Type(value)) if value.name() == "String")
    );
    assert!(
        matches!(resolve(&schema, "@deprecated(reason:)"), Some(ResolvedSchemaCoordinate::DirectiveArgument(value)) if value.name == "reason")
    );
    assert!(
        matches!(resolve(&schema, "@skip(if:)"), Some(ResolvedSchemaCoordinate::DirectiveArgument(value)) if value.name == "if")
    );
}

#[test]
fn unknown_leaf_coordinates_return_none() {
    let schema = schema();
    for source in [
        "Missing",
        "Query.missing",
        "Filter.missing",
        "Status.MISSING",
        "Query.user(missing:)",
        "@missing",
        "@auth(missing:)",
    ] {
        assert!(resolve(&schema, source).is_none(), "source: {source}");
    }
}

#[test]
fn unknown_non_leaf_is_stable_error_and_does_not_return_partial_result() {
    let schema = schema();
    for source in [
        "Missing.field",
        "Missing.field(argument:)",
        "Query.missing(argument:)",
        "@missing(argument:)",
    ] {
        let coordinate = parse_schema_coordinate(SourceId::new(9), source).unwrap();
        let error = resolve_schema_coordinate(&schema, &coordinate).unwrap_err();
        assert_eq!(error.code, SCHEMA_COORDINATE_CONTAINER_MISSING);
    }
}

#[test]
fn rejects_union_member_kind_mismatches() {
    let schema = schema();
    for source in [
        "String.value",
        "Status.ACTIVE(argument:)",
        "Filter.term(argument:)",
        "Search.Query",
        "Search.Query(argument:)",
    ] {
        let coordinate = parse_schema_coordinate(SourceId::new(9), source).unwrap();
        let error = resolve_schema_coordinate(&schema, &coordinate).unwrap_err();
        assert_eq!(error.code, SCHEMA_COORDINATE_INVALID_CONTAINER);
    }
}
