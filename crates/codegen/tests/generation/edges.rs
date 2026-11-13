//! Test generated consumers with large source lists and internal identifiers.

use std::fs;

use super::{TestDirectory, read_files};

#[test]
fn graphql_arguments_compile_with_consumer_internal_name_collisions() {
    let temporary = TestDirectory::new("internal-names");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        r#"
            type Query {
                check(
                    values: String
                    context: String
                    resolvers: String
                    parent: String
                    arguments: String
                    output: String
                    args: String
                    ctx: String
                    self: String
                    crate: String
                    super: String
                ): String
                crateObject: crate
                superObject: super
            }
            type crate { value: String }
            type super { value: String }
            scalar _
            enum Self { _ }
        "#,
    )
    .unwrap();
    temporary.generate();
    temporary.write_consumer("internal_names_consumer", "pub use ::generated;\n");

    temporary.assert_consumer("check");
}

#[test]
fn schema_names_that_normalize_to_builder_symbols_are_disambiguated() {
    let temporary = TestDirectory::new("projection-name-collision");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        "type Query { foo: Foo, fooBuilder: FooBuilder }\n\
         type Foo { id: ID! }\n\
         type FooBuilder { id: ID! }\n",
    )
    .unwrap();

    temporary.generate();
    temporary.write_consumer(
        "projection_name_collision_consumer",
        "pub use ::generated;\n",
    );

    temporary.assert_consumer("check");
}

#[test]
fn resolver_methods_escape_keywords_after_applying_prefixes() {
    let temporary = TestDirectory::new("resolver-method-keyword");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        "type Query { value: type }\ntype type { value: String }\n",
    )
    .unwrap();

    temporary.generate();
    let source = read_files(&generated)
        .into_values()
        .map(|contents| String::from_utf8(contents).unwrap())
        .collect::<String>();

    assert!(source.contains("pub fn with_type<R>"));
    assert!(!source.contains("with_r#type"));
}

#[test]
fn splits_401_sources_and_query_contracts_into_responsibility_owned_files() {
    let temporary = TestDirectory::new("source-shards");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("source_000.graphql"),
        "type Query { field000: String }\n",
    )
    .unwrap();
    for index in 1..401 {
        fs::write(
            schema.join(format!("source_{index:03}.graphql")),
            format!("extend type Query {{ field{index:03}: String }}\n"),
        )
        .unwrap();
    }

    temporary.generate();
    let files = read_files(&generated);
    let sources = std::str::from_utf8(&files["src/__chigiri/sources.rs"]).unwrap();
    assert_eq!(sources.matches("// - source_").count(), 401);
    assert!(sources.contains("source_400.graphql"));
    let query = std::str::from_utf8(&files["src/__chigiri/resolvers/query.rs"]).unwrap();
    assert_eq!(query.matches("fn field").count(), 401);
    assert!(files.contains_key("src/__chigiri/modules.rs"));
    assert_budget(&files);
    temporary.write_consumer("source_shards_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

#[test]
fn splits_large_object_and_scalar_aggregates_for_consumer_compilation() {
    let temporary = TestDirectory::new("schema-aggregation");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    let mut source = String::from("type Query { ok: Boolean! }\n");
    for index in 0..40 {
        source.push_str(&format!("scalar Scalar{index:03}\n"));
    }
    for index in 0..270 {
        source.push_str(&format!("type Object{index:03} {{ value: String }}\n"));
    }
    fs::write(schema.join("schema.graphql"), source).unwrap();

    temporary.generate();
    let files = read_files(&generated);
    assert_budget(&files);
    let stage_impls = files
        .values()
        .map(|contents| String::from_utf8_lossy(contents))
        .map(|contents| {
            contents
                .matches("impl<C, T, E> super::ResolverStage")
                .count()
        })
        .sum::<usize>();
    assert_eq!(stage_impls, 271);
    let assembly = std::str::from_utf8(&files["src/__chigiri/assembly.rs"]).unwrap();
    assert_eq!(
        assembly
            .matches("impl<C, T, E> super::ResolverStage")
            .count(),
        271
    );
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/resolvers/objects/"))
            .count(),
        270
    );
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/internal/scalars/types/"))
            .count(),
        40
    );
    temporary.write_consumer("schema_aggregation_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

#[test]
fn descriptions_and_source_names_are_safely_escaped_for_consumer_compilation() {
    let temporary = TestDirectory::new("bidi");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema-\u{202e}.graphql"),
        "\"direction \\u202E marker\"\ntype Query { value: String }\n",
    )
    .unwrap();

    temporary.generate();
    let files = read_files(&temporary.join("generated"));
    assert!(
        files
            .iter()
            .filter(|(name, _)| name.ends_with(".rs"))
            .all(|(_, contents)| !contents.windows(3).any(|part| part == [0xe2, 0x80, 0xae]))
    );
    // Rust source escapes the input name, while ownership metadata preserves
    // the exact original name.
    let marker: serde_json::Value =
        serde_json::from_slice(&files["chigiri-generated.json"]).unwrap();
    assert_eq!(
        marker["provenance"]["sources"][0]["path"],
        "schema-\u{202e}.graphql"
    );
    temporary.write_consumer("bidi_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

#[test]
fn rejects_invalid_names_before_rendering_starts() {
    let temporary = TestDirectory::new("long-name");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    let name = format!("Object{}", "A".repeat(4_096));
    let source = format!("type Query {{ value: {name} }}\ntype {name} {{ value: String }}\n");
    fs::write(schema.join("schema.graphql"), source).unwrap();

    let error = temporary.render_error();

    assert!(error.message().contains("SF4010"));
    assert!(error.message().contains("GraphQL name"));
    assert!(error.message().contains("max=255"));
}

#[test]
fn renderer_rejects_oversized_compressed_multi_source_default_lists_before_starting() {
    let temporary = TestDirectory::new("large-default-list");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    let values = std::iter::repeat_n("0", 10_000)
        .collect::<Vec<_>>()
        .join(",");
    for index in 0..4 {
        let query = if index == 0 {
            "type Query { value: String }\n"
        } else {
            ""
        };
        let source = format!("{query}input Input{index} {{ values: [Int] = [{values}] }}\n");
        fs::write(schema.join(format!("schema_{index}.graphql")), source).unwrap();
    }

    let error = temporary.render_error();

    assert!(error.message().contains("SF4010"));
    assert!(error.message().contains("generated output"));
}

#[test]
fn large_interface_output_is_split_by_owner_and_compiles_for_consumers() {
    let temporary = TestDirectory::new("descriptor-lists");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    let arguments = (0..80)
        .map(|index| format!("argument{index:03}: String"))
        .collect::<Vec<_>>()
        .join(", ");
    let interfaces = (0..401)
        .map(|index| format!("Interface{index:03}"))
        .collect::<Vec<_>>();
    let mut source = format!("directive @wide({arguments}) on OBJECT\n");
    for interface in &interfaces {
        source.push_str(&format!("interface {interface} {{ value: String }}\n"));
    }
    source.push_str("type Query { object: Object }\n");
    source.push_str(&format!(
        "type Object implements {} {{ value: String }}\n",
        interfaces.join(" & ")
    ));
    fs::write(schema.join("schema.graphql"), source).unwrap();

    temporary.generate();
    let files = read_files(&generated);
    assert_budget(&files);
    let all = files
        .values()
        .map(|contents| String::from_utf8_lossy(contents))
        .collect::<String>();
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/types/interface"))
            .count(),
        401
    );
    assert!(files.contains_key("src/__chigiri/internal/descriptor/types/object.rs"));
    assert!(files.contains_key("src/__chigiri/internal/descriptor/directives/wide.rs"));
    assert!(all.contains("INTERFACE_CHUNK_0006"));
    assert!(all.contains("DIRECTIVE_DESCRIPTOR_0000_ARGUMENT_CHUNK_0001"));
    temporary.write_consumer("descriptor_lists_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

#[test]
fn splits_a_400_item_constant_list_into_a_descriptor_that_compiles_for_consumers() {
    let temporary = TestDirectory::new("const-value-descriptor-lists");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    let values = std::iter::repeat_n("0", 401).collect::<Vec<_>>().join(", ");
    fs::write(
        schema.join("schema.graphql"),
        format!(
            "input Nested {{ values: [Int] }}\ninput Filter {{ nested: Nested = {{ values: [{values}] }} }}\ntype Query {{ value(filter: Filter): String }}\n"
        ),
    )
    .unwrap();

    temporary.generate();
    let files = read_files(&generated);
    assert_budget(&files);
    let all = files
        .values()
        .map(|contents| String::from_utf8_lossy(contents))
        .collect::<String>();
    assert!(files.contains_key("src/__chigiri/internal/descriptor/types/filter.rs"));
    assert!(all.contains("DEFAULT_FIELD_LIST_0000"));
    assert!(all.contains("DEFAULT_FIELD_0000_VALUE_ITEM_CHUNK_0006"));
    temporary.write_consumer("const_value_descriptor_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

fn assert_budget(files: &std::collections::BTreeMap<String, Vec<u8>>) {
    assert!(files.len() <= 65_536);
    assert!(files.values().map(Vec::len).sum::<usize>() <= 64 * 1024 * 1024);
    assert!(!files.keys().any(|name| name.ends_with("_00.rs")));
}
