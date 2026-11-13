use super::{SCHEMA_DUPLICATE_MEMBER, SchemaOrigin, TypeKind, compose_schema};
use crate::language::{ast::Document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(
        output.is_valid(),
        "SDL parse failed: {:?}",
        output.diagnostics.into_vec()
    );
    output.document.unwrap()
}

#[test]
fn extensions_may_precede_definitions_across_documents() {
    let extension = parse(
        11,
        r#"
        extend type Query { second: String }
        scalar Date
        "#,
    );
    let definitions = parse(
        12,
        r#"
        type Query { first: Int }
        type User { id: ID! }
        extend type Query { third: User }
        "#,
    );

    let output = compose_schema(&[extension, definitions], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let schema = output.schema.unwrap();
    let names = schema
        .types()
        .iter()
        .map(|named| named.name())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "Int", "Float", "String", "Boolean", "ID", "Date", "Query", "User"
        ]
    );
    let query = schema.type_by_name("Query").unwrap();
    assert_eq!(query.kind(), TypeKind::Object);
    assert_eq!(
        query
            .fields()
            .iter()
            .map(|field| field.name.value.as_str())
            .collect::<Vec<_>>(),
        ["second", "first", "third"]
    );
    assert_eq!(
        query.field("second").unwrap().name.span.source_id,
        SourceId::new(11)
    );
    assert!(
        matches!(query.origins()[0], SchemaOrigin::Extension(span) if span.source_id == SourceId::new(11))
    );
    assert!(
        matches!(query.origins()[1], SchemaOrigin::Definition(span) if span.source_id == SourceId::new(12))
    );
    assert!(
        matches!(query.origins()[2], SchemaOrigin::Extension(span) if span.source_id == SourceId::new(12))
    );
}

#[test]
fn distinguishes_implicit_and_explicit_root_origins() {
    let implicit = compose_schema(
        &[parse(21, "type Query { ok: Boolean }")],
        &ExecutionLimits::unbounded(),
    )
    .schema
    .unwrap();
    assert_eq!(
        implicit.roots().query().unwrap().origin,
        SchemaOrigin::Implicit
    );

    let explicit = compose_schema(
        &[parse(
            22,
            "schema { query: Root } type Root { ok: Boolean } type Query { unused: ID }",
        )],
        &ExecutionLimits::unbounded(),
    )
    .schema
    .unwrap();
    let root = explicit.roots().query().unwrap();
    assert_eq!(root.type_name, "Root");
    assert!(
        matches!(root.origin, SchemaOrigin::Definition(span) if span.source_id == SourceId::new(22))
    );
}

#[test]
fn resolves_custom_directives_with_signatures_and_source_origins() {
    let document = parse(
        31,
        r#"
        directive @tag(label: String = "x") repeatable on OBJECT | FIELD_DEFINITION
        type Query @tag @tag { value: String @tag }
        "#,
    );
    let schema = compose_schema(&[document], &ExecutionLimits::unbounded())
        .schema
        .unwrap();
    let directive = schema.directive_definition("tag").unwrap();

    assert!(directive.is_repeatable());
    assert_eq!(directive.arguments()[0].name, "label");
    assert!(directive.source_definition().is_some());
    assert!(
        matches!(directive.origin(), SchemaOrigin::Definition(span) if span.source_id == SourceId::new(31))
    );
    assert_eq!(schema.type_by_name("Query").unwrap().directives().len(), 2);
}

#[test]
fn rejects_duplicate_directive_locations_at_the_second_span() {
    let document = parse(
        32,
        "directive @audit on FIELD | FIELD type Query { value: String }",
    );
    let expected_span = document
        .type_system_definitions()
        .find_map(|definition| match definition {
            crate::language::ast::TypeSystemDefinition::Directive(directive) => {
                Some(directive.locations[1].span)
            }
            _ => None,
        })
        .unwrap();
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_MEMBER)
        .unwrap();

    assert!(output.schema.is_none());
    assert_eq!(diagnostic.span, Some(expected_span));
}

#[test]
fn merges_minimal_extensions_into_the_incremental_index() {
    const EXTENSIONS: usize = 2_000;
    let mut source = String::with_capacity(EXTENSIONS * 100);
    source.push_str(
        "type Query { base(arg: Int): String }\n\
         enum State { BASE }\n\
         input Filter { base: String }\n",
    );
    for index in 0..EXTENSIONS {
        source.push_str(&format!(
            "extend type Query {{ f{index}(a: Int): String }}\n"
        ));
        source.push_str(&format!("extend enum State {{ V{index} }}\n"));
        source.push_str(&format!("extend input Filter {{ f{index}: String }}\n"));
    }
    let document = parse(33, &source);
    let schema = compose_schema(&[document], &ExecutionLimits::unbounded())
        .schema
        .unwrap();
    let query = schema.type_by_name("Query").unwrap();
    let state = schema.type_by_name("State").unwrap();
    let filter = schema.type_by_name("Filter").unwrap();

    assert_eq!(query.fields().len(), EXTENSIONS + 1);
    assert!(query.field("f1999").is_some());
    assert!(query.field_argument("f1999", "a").is_some());
    assert!(state.enum_value("V1999").is_some());
    assert!(filter.input_field("f1999").is_some());
}
