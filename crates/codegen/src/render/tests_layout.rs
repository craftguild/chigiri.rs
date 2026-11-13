//! Keeps large public contracts in ownership units and localizes unrelated schema changes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use chigiri_core::language::parse_type_system_document;
use chigiri_core::schema::compose_schema;
use chigiri_core::{ExecutionLimits, SourceMap};

use crate::formatter::format_items;
use crate::writer::{GeneratedFile, assemble_items};

use super::dispatch::render_dispatch_items;
use super::model::RenderModel;
use super::resolvers::render_resolver_items;

#[test]
fn sg_layout_01_renders_eighty_field_resolvers_in_one_public_trait() {
    let mut source = String::from("type Query { wide: Wide! }\ntype Wide {\n");
    for index in 0..80 {
        writeln!(&mut source, "  field{index:03}: String!").unwrap();
    }
    source.push_str("}\n");
    let files = render_targeted(&source);
    let resolver = &files["resolvers/objects/wide.rs"];
    assert_eq!(resolver.matches("pub trait WideResolver").count(), 1);
    assert!(resolver.contains("fn field000"));
    assert!(resolver.contains("fn field079"));
    assert!(resolver.lines().count() > 400);
    assert!(!resolver.contains("macro_rules!"));
    assert!(files.contains_key("assembly.rs"));
    assert!(files.contains_key("internal/dispatch/wide.rs"));
}

#[test]
fn sg_layout_03_long_nested_types_keep_methods_together() {
    let nested = format!("{}String!{}", "[".repeat(40), "]!".repeat(40));
    let mut source = String::from("type Query { wide: Wide! }\ntype Wide {\n");
    for index in 0..30 {
        writeln!(&mut source, "  nested{index:02}: {nested}").unwrap();
    }
    source.push_str("}\n");
    let files = render_targeted(&source);
    let resolver = &files["resolvers/objects/wide.rs"];
    assert_eq!(resolver.matches("fn nested").count(), 30);
    assert_eq!(resolver.matches("::std::vec::Vec<").count(), 30 * 40);
    assert!(!resolver.contains("macro_rules!"));
}

#[test]
fn sg_layout_03_derives_roles_from_actual_operation_roots() {
    let source = "schema { query: Read mutation: Write } type Read { item: Query } type Write { value: Int } type Query { value: Int }";
    let files = render_targeted(source);
    assert!(files["resolvers/query.rs"].contains("trait ReadResolver"));
    assert!(files["resolvers/mutation.rs"].contains("trait WriteResolver"));
    assert!(files["resolvers/objects/query.rs"].contains("trait QueryResolver"));
    assert!(!files.keys().any(|path| path.ends_with("_00.rs")));
}

#[test]
fn sg_layout_04_unrelated_growth_does_not_change_existing_model_and_trait_paths_or_bytes() {
    let source = "type Query { book: Book } type Book { id: ID! } enum State { OPEN CLOSED } input Filter { state: State }";
    let first = render_targeted(source);
    assert_eq!(first, render_targeted(source));
    let added =
        "enum Before { A B } type Independent { field: Int } input Unrelated { name: String }";
    let second = render_targeted(&format!("{added} {source}"));
    for path in [
        "types/state.rs",
        "types/filter.rs",
        "resolvers/objects/book.rs",
        "resolvers/query.rs",
    ] {
        assert_eq!(first[path], second[path], "{path}");
    }
    assert_ne!(first["assembly.rs"], second["assembly.rs"]);
    assert_ne!(first["schema_types.rs"], second["schema_types.rs"]);
    let third = render_targeted(&format!(
        "{} {source}",
        added.replace("field: Int", "field: Int another: String")
    ));
    assert_eq!(
        second["resolvers/objects/book.rs"],
        third["resolvers/objects/book.rs"]
    );
}

fn render_targeted(source: &str) -> BTreeMap<String, String> {
    let mut source_map = SourceMap::new();
    let source_id = source_map.add("wide.graphql", source);
    let parsed = parse_type_system_document(source_id, source, &ExecutionLimits::unbounded());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let composed = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(
        composed.diagnostics.is_empty(),
        "{:?}",
        composed.diagnostics
    );
    let schema = composed.schema.unwrap();
    let model = RenderModel::build_with_sources(&schema, &source_map).unwrap();
    super::layout::validate(&model).unwrap();
    let mut items = render_resolver_items(&model).unwrap();
    items.extend(super::types::render_type_items(&model).unwrap());
    let dispatch = render_dispatch_items(&model);
    items.extend(dispatch.definitions);
    items.extend(dispatch.methods);
    assemble_items(format_items(items).unwrap())
        .unwrap()
        .into_iter()
        .map(|GeneratedFile { name, contents, .. }| (name, contents))
        .collect()
}
