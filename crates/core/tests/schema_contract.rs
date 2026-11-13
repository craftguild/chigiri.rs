use chigiri_core::language::parse_type_system_document;
use chigiri_core::schema::{
    ResolvedSchemaCoordinate, SchemaCoordinate, SchemaOrigin, TypeKind, compose_schema,
    parse_schema_coordinate, resolve_schema_coordinate,
};
use chigiri_core::{ExecutionLimits, SourceId};

#[test]
fn public_api_composes_multiple_parsed_documents() {
    let limits = ExecutionLimits::unbounded();
    let first = parse_type_system_document(SourceId::new(101), "type Query { id: ID! }", &limits)
        .document
        .unwrap();
    let second = parse_type_system_document(
        SourceId::new(102),
        "extend type Query { name: String }",
        &limits,
    )
    .document
    .unwrap();

    let output = compose_schema(&[first, second], &limits);
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let schema = output.schema.unwrap();
    let query = schema.type_by_name("Query").unwrap();
    assert_eq!(query.kind(), TypeKind::Object);
    assert_eq!(query.fields().len(), 2);
    assert_eq!(
        schema.roots().query().unwrap().origin,
        SchemaOrigin::Implicit
    );
}

#[test]
fn public_api_parses_and_resolves_schema_coordinates() {
    let limits = ExecutionLimits::unbounded();
    let document = parse_type_system_document(
        SourceId::new(103),
        "type Query { user(id: ID!): String }",
        &limits,
    )
    .document
    .unwrap();
    let schema = compose_schema(&[document], &limits).schema.unwrap();
    let coordinate = parse_schema_coordinate(SourceId::new(104), "Query.user(id:)").unwrap();

    assert!(matches!(coordinate, SchemaCoordinate::FieldArgument { .. }));
    assert!(matches!(
        resolve_schema_coordinate(&schema, &coordinate).unwrap(),
        Some(ResolvedSchemaCoordinate::FieldArgument(argument)) if argument.name.value == "id"
    ));
}
