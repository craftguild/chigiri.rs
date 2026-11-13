//! Tests static schema-descriptor rendering.

use chigiri_core::language::parse_type_system_document;
use chigiri_core::schema::compose_schema;
use chigiri_core::{ExecutionLimits, SourceId};

use crate::writer::assemble_items;

use super::{RenderModel, render_descriptor_items};

fn render(source: &str) -> String {
    let limits = ExecutionLimits::unbounded();
    let parsed = parse_type_system_document(SourceId::new(1), source, &limits);
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let composed = compose_schema(&[parsed.document.unwrap()], &limits);
    assert!(composed.is_valid(), "{:?}", composed.diagnostics.into_vec());
    let model = RenderModel::build(composed.schema.as_ref().unwrap()).unwrap();
    assemble_items(
        render_descriptor_items(&model, chigiri_core::SchemaDigest::from_bytes([0; 32])).unwrap(),
    )
    .unwrap()
    .into_iter()
    .map(|file| file.contents)
    .collect()
}

#[test]
fn static_descriptors_match_canonical_schema_metadata() {
    let source = r#"
        directive @tag(level: Int = 1) repeatable on OBJECT | FIELD_DEFINITION
        directive @schemaTag(value: String!) on SCHEMA
        "descriptor schema"
        schema @schemaTag(value: "kept") { query: Query }
        scalar URL @specifiedBy(url: "https://example.com/url")
        interface Node { id: ID! }
        type User implements Node @tag(level: 2) { id: ID! old: String @deprecated }
        union Search = User | Query
        enum Color { RED OLD @deprecated(reason: "Use RED") }
        input Choice @oneOf { id: ID, name: String }
        input Filter { color: Color = RED }
        type Query { search(filter: Filter): [Search!]! }
    "#;
    let generated = render(source);

    assert!(generated.contains("possible_types: super::TYPE_DESCRIPTOR_"));
    assert!(generated.contains("POSSIBLE_TYPE_LIST_0000"));
    assert!(generated.contains("\"User\","));
    assert!(generated.contains("specified_by_url: Some(\"https://example.com/url\")"));
    assert!(generated.contains("deprecation_reason: Some(\"No longer supported\")"));
    assert!(generated.contains("default_value: Some(\"RED\")"));
    assert!(generated.contains("is_one_of: true"));
    assert!(generated.contains("\"FIELD_DEFINITION\""));
    assert!(generated.contains("super::TYPE_DESCRIPTOR_0000"));
    assert!(generated.contains("description: Some(\"descriptor schema\")"));
    assert!(generated.contains("schema_directives: super::SCHEMA_DIRECTIVE_LIST_0000"));
    assert!(generated.contains("value: \"\\\"kept\\\"\""));
}

#[test]
fn splits_large_field_member_sets_into_independent_items() {
    let fields = (0..80)
        .map(|index| format!("field{index}: String!"))
        .collect::<Vec<_>>()
        .join(" ");
    let generated = render(&format!("type Query {{ {fields} }}"));

    assert!(generated.contains("TYPE_DESCRIPTOR_0000_FIELD_0079"));
    assert!(generated.contains("fields: super::TYPE_DESCRIPTOR_0000_FIELD_LIST_0000"));
    assert!(generated.contains("TYPE_DESCRIPTOR_0000_FIELD_CHUNK_0001"));
}

#[test]
fn builtin_scalar_extension_applications_preserve_composed_source_order() {
    let source = r#"
        directive @tag(value: Int!) repeatable on SCALAR
        extend scalar String @tag(value: 1)
        extend scalar String @tag(value: 2)
        type Query { value: String }
    "#;
    let generated = render(source);

    assert!(generated.contains("BUILT_IN_SCALAR_OVERLAY_0000"));
    assert!(generated.contains("name: \"String\""));
    assert!(generated.contains("built_in_scalar_overlays:"));
    let first = generated.find("value: \"1\"").unwrap();
    let second = generated.find("value: \"2\"").unwrap();
    assert!(first < second);
}

#[test]
fn large_type_and_directive_argument_sets_are_split_into_fixed_width_lists() {
    let arguments = (0..80)
        .map(|index| format!("argument{index:03}: String"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = format!(
        "directive @wide({arguments}) on OBJECT\ninterface Node {{ value: String }}\ntype Query {{ value: String }}\n"
    );
    for index in 0..401 {
        source.push_str(&format!(
            "type Object{index:03} implements Node {{ value: String }}\n"
        ));
    }

    let generated = render(&source);

    assert!(generated.contains("POSSIBLE_TYPE_CHUNK_0006"));
    assert!(generated.contains("\"Object400\""));
    assert!(generated.contains("DIRECTIVE_DESCRIPTOR_0000_ARGUMENT_CHUNK_0001"));
    assert!(generated.contains("DIRECTIVE_DESCRIPTOR_0000_ARGUMENT_0079"));
}

#[test]
fn descriptors_preserve_definition_and_preceding_extension_order() {
    let source = r#"
        extend enum State { BEFORE }
        extend input Filter { inputBefore: String }
        extend union Search = Before
        extend type Item implements Early { objectBefore: String }
        enum State { AFTER }
        input Filter { inputAfter: String }
        type Before { value: String }
        type After { value: String }
        union Search = After
        interface Early { objectBefore: String }
        interface Late { objectAfter: String }
        type Item implements Late { objectAfter: String }
        type Query { item: Item, search: Search, filter(value: Filter): State }
    "#;
    let generated = render(source);

    assert!(
        generated.find("name: \"BEFORE\"").unwrap() < generated.find("name: \"AFTER\"").unwrap()
    );
    assert!(
        generated.find("name: \"inputBefore\"").unwrap()
            < generated.find("name: \"inputAfter\"").unwrap()
    );
    assert!(
        generated.find("name: \"objectBefore\"").unwrap()
            < generated.find("name: \"objectAfter\"").unwrap()
    );
    assert!(generated.contains("\"Before\",\n    \"After\","));
    assert!(generated.contains("\"Early\",\n    \"Late\","));
}
