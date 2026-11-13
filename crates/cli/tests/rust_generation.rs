#[path = "../../codegen/tests/support/cargo.rs"]
mod cargo;
#[path = "../../codegen/tests/generation/edges.rs"]
mod generation_edges;
#[path = "../../codegen/tests/generation/fixture.rs"]
mod generation_fixture;
#[path = "../../codegen/tests/generation/subscription.rs"]
mod generation_subscription;
#[path = "../../codegen/tests/generation/subscription_shapes.rs"]
mod generation_subscription_shapes;

use generation_fixture::{TestDirectory, fixture, read_files};
use std::fs;

#[test]
fn identical_schemas_generate_byte_identical_files_and_inventories() {
    let first = TestDirectory::new("deterministic-first");
    let second = TestDirectory::new("deterministic-second");
    let schema = fixture("tests/fixtures/full");
    for temporary in [&first, &second] {
        temporary.copy_schema(&schema);
        temporary.generate();
    }
    assert_eq!(
        read_files(&first.join("generated")),
        read_files(&second.join("generated"))
    );
    chigiri_codegen::check_contract(&first.manifest()).unwrap();
}

#[test]
fn checked_in_golden_is_consistent_with_schema() {
    let manifest =
        chigiri_codegen::ResolvedManifest::load(&fixture("tests/chigiri-full.toml")).unwrap();
    chigiri_codegen::check_contract(&manifest).unwrap();
}

#[test]
fn custom_scalar_output_contract_can_be_executed_on_generated_paths() {
    let manifest = chigiri_codegen::ResolvedManifest::load(&fixture(
        "tests/chigiri-custom-scalar-output.toml",
    ))
    .unwrap();
    chigiri_codegen::check_contract(&manifest).unwrap();
    assert_fixture("custom_scalar_output", true, &[]);
}

#[test]
fn server_codegen_rejects_client_artifact_configuration() {
    let temporary = TestDirectory::new("server-only");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        "type Query { health: String! }\n",
    )
    .unwrap();

    temporary.generate();
    let output = read_files(&generated);
    let joined = output
        .values()
        .flat_map(|contents| contents.iter().copied())
        .collect::<Vec<_>>();
    let joined = String::from_utf8(joined).unwrap();

    assert!(joined.contains("pub trait QueryResolver"));
    assert!(joined.contains("pub static SCHEMA_DESCRIPTOR"));
    assert!(!joined.contains("PersistedOperation"));
    assert!(!joined.contains("OperationCatalog"));
}

#[test]
fn invalid_sdl_does_not_change_the_existing_output() {
    let temporary = TestDirectory::new("invalid");
    let schema = temporary.join("schema");
    let output = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        "type Query { value: String }",
    )
    .unwrap();
    temporary.generate();
    let before = read_files(&output);
    fs::write(schema.join("schema.graphql"), "type Query { broken: }\n").unwrap();
    let result = temporary.cli("update");
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("SF1"));
    assert_eq!(read_files(&output), before);
}

#[test]
fn generated_public_contracts_compile_and_missing_ones_are_compile_errors() {
    assert_fixture("pass", true, &[]);
    assert_fixture(
        "field_missing",
        false,
        &["not all trait items implemented", "health"],
    );
    assert_fixture(
        "group_missing",
        false,
        &["no method named `build`", "ResolverStage"],
    );
}

#[test]
fn res_02_03_typestate_violation_is_stable_fragment_compile_failed() {
    assert_typestate_fixture(
        "duplicate",
        &["no method named `with_user`", "ResolverStage"],
    );
    assert_typestate_fixture(
        "wrong_order",
        &["no method named `with_query`", "ResolverStage"],
    );
    assert_typestate_fixture("wrong_trait", &["UserResolver"]);
    assert_typestate_fixture("stage_clone", &["no method named `clone`", "ResolverStage"]);
}

#[test]
fn consumer_crate_exposes_expanded_semantic_units() {
    let temporary = TestDirectory::new("method-macro");
    let schema = temporary.join("schema");
    let generated = temporary.join("generated");
    fs::create_dir(&schema).unwrap();
    let mut source = String::from("type Query { wide: Wide! }\ntype Wide {\n");
    for index in 0..80 {
        source.push_str(&format!("  field{index:03}: String!\n"));
    }
    source.push_str("}\n");
    fs::write(schema.join("schema.graphql"), source).unwrap();

    temporary.generate();
    let files = read_files(&generated);
    let resolver = std::str::from_utf8(&files["src/__chigiri/resolvers/objects/wide.rs"]).unwrap();
    assert_eq!(resolver.matches("fn field").count(), 80);
    assert!(resolver.lines().count() > 400);
    assert!(!resolver.contains("macro_rules!"));
    temporary.write_consumer("semantic_resolver_consumer", "pub use ::generated;\n");
    temporary.assert_consumer("check");
}

#[test]
fn dynamically_resolves_root_objects_at_root_and_nested_positions() {
    let temporary = TestDirectory::new("root-nested");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        "type Query { child: Query, value: String! }\n",
    )
    .unwrap();
    temporary.generate();
    let source = fs::read_to_string(fixture("tests/compile/root_nested.rs")).unwrap();
    temporary.write_consumer("root_nested_consumer", &source);
    temporary.assert_consumer("test");
}

#[test]
fn compiles_recursive_inputs_and_consumer_prelude_types() {
    let temporary = TestDirectory::new("input-edges");
    let schema = temporary.join("schema");
    fs::create_dir(&schema).unwrap();
    fs::write(
        schema.join("schema.graphql"),
        r#"
            enum Option { VALUE }
            enum Vec { VALUE }
            enum Send { VALUE }
            input Recursive { next: Recursive }
            input A { b: B }
            input B { a: A }
            input NonNullA { b: NonNullB! }
            input NonNullB { a: NonNullA }
            input Choice @oneOf { next: Choice, id: ID }
            input Filter {
                text: String = "x"
                values: [String!]!
                recursive: Recursive
                a: A
                nonNullA: NonNullA
                choice: Choice
            }
            type Query { ok(filter: Filter): Boolean! }
        "#,
    )
    .unwrap();
    temporary.generate();
    let generated = read_files(&temporary.join("generated"));
    assert!(
        generated
            .values()
            .any(|source| { String::from_utf8_lossy(source).contains("std::boxed::Box") })
    );
    assert!(generated.values().any(|source| {
        String::from_utf8_lossy(source).contains("pub b: ::std::boxed::Box<super::NonNullB>,")
    }));
    let source = fs::read_to_string(fixture("tests/compile/input_edges.rs")).unwrap();
    temporary.write_consumer("input_edges_consumer", &source);
    temporary.assert_consumer("check");
}

fn assert_fixture(name: &str, success: bool, expected: &[&str]) {
    let manifest = fixture(&format!("tests/compile/{name}/Cargo.toml"));
    let action = if success { "test" } else { "check" };
    let output = cargo::run(&mut cargo::command(&manifest, action));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), success, "{stderr}");
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "stderr must contain `{fragment}`"
        );
    }
}

fn assert_typestate_fixture(binary: &str, expected: &[&str]) {
    let manifest = fixture("tests/compile/typestate_fail/Cargo.toml");
    let output = cargo::run(cargo::command(&manifest, "check").args(["--bin", binary]));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    for fragment in expected {
        assert!(
            stderr.contains(fragment),
            "stderr must contain `{fragment}`"
        );
    }
}
