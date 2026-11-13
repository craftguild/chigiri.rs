//! Tests standalone generation boundaries with independent domain, generated, and consumer crates in Cargo.

mod sibling_support;

use std::fmt::Write as _;
use std::fs;

use chigiri_codegen::{GENERATED_MARKER_FILE, ResolvedManifest, render_contract};
use sibling_support::Fixture;

const SCHEMA: &str = include_str!("fixtures/sibling-enum/schema.graphql");
const CONSUMER: &str = include_str!("fixtures/sibling-enum/consumer.rs");

#[test]
fn sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately() {
    let fixture = Fixture::new();
    fixture.prepare(SCHEMA, &["AdapterKind"]);
    let manifest = ResolvedManifest::load(&fixture.0.join("chigiri.toml")).unwrap();
    let rendered = render_contract(&manifest).unwrap();
    fixture.write_rendered(&rendered);
    fixture.format_check(&rendered);
    let marker: serde_json::Value =
        serde_json::from_slice(fixture.file(&rendered, GENERATED_MARKER_FILE)).unwrap();
    let info =
        chigiri_codegen::read_generator_info(&serde_json::to_vec(&marker["generator"]).unwrap())
            .unwrap();
    assert_eq!(&info, chigiri_codegen::generator_info());
    assert_eq!(marker["abi"], 8);
    assert_eq!(
        marker["files"].as_array().unwrap().len() + 1,
        rendered.files().len()
    );
    let cargo = std::str::from_utf8(fixture.file(&rendered, "Cargo.toml")).unwrap();
    assert!(cargo.contains("[workspace]"));
    assert!(cargo.contains("publish = false"));
    assert!(!cargo.contains(fixture.0.to_str().unwrap()));
    fixture.cargo("private/contract/Cargo.toml", &["check"], true);
    fixture.cargo(
        "private/contract/Cargo.toml",
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        true,
    );
    fixture.cargo("private/contract/Cargo.toml", &["doc", "--no-deps"], true);
    fixture.prepare_consumer(CONSUMER);
    fixture.cargo("consumer/Cargo.toml", &["test"], true);
    assert!(!fixture.0.join("generated/contract/Cargo.lock").exists());
    assert!(!fixture.0.join("generated/contract/target").exists());

    let field_start = CONSUMER.find("    async fn echo").unwrap();
    let field_end = CONSUMER.find("    async fn item").unwrap();
    let missing_field = format!("{}{}", &CONSUMER[..field_start], &CONSUMER[field_end..]);
    fs::write(fixture.0.join("consumer/src/lib.rs"), missing_field).unwrap();
    let error = fixture.cargo("consumer/Cargo.toml", &["check"], false);
    assert!(error.contains("E0046") && error.contains("echo"));
    let types_and_query = CONSUMER.split("pub fn resolvers").next().unwrap();
    for (tail, code) in [
        (
            "pub fn probe() { let _ = contract::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>).with_query(Query).build(); }",
            "E0599",
        ),
        (
            "pub struct MissingItem; impl contract::ItemResolver<(), Types, chigiri::FieldError> for MissingItem {}",
            "E0046",
        ),
        (
            "pub fn probe() -> Option<contract::__chigiri::AdapterKind> { None }",
            "E0603",
        ),
    ] {
        fs::write(
            fixture.0.join("consumer/src/lib.rs"),
            format!("{types_and_query}{tail}"),
        )
        .unwrap();
        let error = fixture.cargo("consumer/Cargo.toml", &["check"], false);
        assert!(error.contains(code), "{error}");
    }

    fs::write(fixture.0.join("consumer/src/lib.rs"), "pub struct Types; impl contract::SchemaTypes for Types { type Item = external_domain::Item; }").unwrap();
    let missing = fixture.cargo("consumer/Cargo.toml", &["check"], false);
    assert!(missing.contains("E0046") && missing.contains("adapter_kind_from_graphql"));
    // Inject a malformed candidate and verify that its ABI 7 guard cannot link to runtime 8.
    let root = fixture.file(&rendered, "src/lib.rs");
    let old_abi = std::str::from_utf8(root)
        .unwrap()
        .replace("GENERATED_ABI_VERSION == 8", "GENERATED_ABI_VERSION == 7");
    fs::write(fixture.0.join("private/contract/src/lib.rs"), old_abi).unwrap();
    let old = fixture.cargo("private/contract/Cargo.toml", &["check"], false);
    assert!(old.contains("E0080") && old.contains("ABI mismatch"));
    fs::write(fixture.0.join("private/contract/src/lib.rs"), root).unwrap();
    fs::write(fixture.0.join("consumer/src/lib.rs"), CONSUMER).unwrap();
    fs::write(
        fixture.0.join("graphql/schema.graphql"),
        SCHEMA.replace("V3 V4", "V3 V4 V5"),
    )
    .unwrap();
    fixture.write_rendered(&render_contract(&manifest).unwrap());
    fixture.cargo("private/contract/Cargo.toml", &["check"], true);
    let added = fixture.cargo("consumer/Cargo.toml", &["check"], false);
    assert!(added.contains("E0004") && added.contains("V5"));
}

#[test]
fn sg_crate_04_keyword_and_metadata_name_and_stdlib_shadow_standalone() {
    let fixture = Fixture::new();
    fixture.prepare(
        "enum GeneratorInfo { CURRENT } enum Type { TYPE SELF ASYNC } enum Self { YES } enum Async { YES } type std { value: String! } type chigiri { value: String! } type generator { value: String! } type Query { std: std! chigiri: chigiri! generator: generator! info: GeneratorInfo! kind: Type! own: Self! future: Async! }",
        &["Type", "Self", "Async"],
    );
    let manifest = ResolvedManifest::load(&fixture.0.join("chigiri.toml")).unwrap();
    let rendered = render_contract(&manifest).unwrap();
    fixture.write_rendered(&rendered);
    fixture.cargo(
        "private/contract/Cargo.toml",
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        true,
    );
    fixture.cargo("private/contract/Cargo.toml", &["doc", "--no-deps"], true);
}

#[test]
fn sg_layout_01_splits_large_query_mutation_input_and_enum_outputs_by_semantic_unit() {
    let fixture = Fixture::new();
    let mut fields = String::new();
    for index in 0..240 {
        writeln!(fields, "field{index}: String!").unwrap();
    }
    let mut input = String::new();
    let mut variants = String::new();
    for index in 0..420 {
        writeln!(input, "field{index}: Int").unwrap();
        writeln!(variants, "VALUE_{index}").unwrap();
    }
    fixture.prepare(&format!("type Query {{\n{fields}}} type Mutation {{\n{fields}}} input HugeInput {{\n{input}}} enum HugeEnum {{\n{variants}}}"), &[]);
    let manifest = ResolvedManifest::load(&fixture.0.join("chigiri.toml")).unwrap();
    let rendered = render_contract(&manifest).unwrap();
    for role in ["query", "mutation"] {
        let path = format!("src/__chigiri/resolvers/{role}.rs");
        let declarations = std::str::from_utf8(fixture.file(&rendered, &path)).unwrap();
        assert_eq!(declarations.matches("fn field").count(), 240);
        assert!(declarations.lines().count() > 400);
        assert!(!declarations.contains("macro_rules!"));
    }
    for owner in ["huge_input", "huge_enum"] {
        let path = format!("src/__chigiri/types/{owner}.rs");
        assert!(
            std::str::from_utf8(fixture.file(&rendered, &path))
                .unwrap()
                .lines()
                .count()
                > 400
        );
    }
    fixture.write_rendered(&rendered);
    fixture.format_check(&rendered);
    chigiri_codegen::check_contract(&manifest).unwrap();
    fixture.cargo(
        "private/contract/Cargo.toml",
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        true,
    );
    fixture.cargo("private/contract/Cargo.toml", &["doc", "--no-deps"], true);
}

#[test]
fn sg_crate_06_manifest_comment_changes_provenance_while_preserving_semantic_binding() {
    let fixture = Fixture::new();
    fixture.prepare(SCHEMA, &["AdapterKind"]);
    let path = fixture.0.join("chigiri.toml");
    let first = render_contract(&ResolvedManifest::load(&path).unwrap()).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("# harmless comment\n{source}")).unwrap();
    let second = render_contract(&ResolvedManifest::load(&path).unwrap()).unwrap();
    let marker = |rendered: &chigiri_codegen::RenderedContract| {
        serde_json::from_slice::<serde_json::Value>(fixture.file(rendered, GENERATED_MARKER_FILE))
            .unwrap()
    };
    let left = marker(&first);
    let right = marker(&second);
    assert_eq!(
        left["provenance"]["schema_sha256"],
        right["provenance"]["schema_sha256"]
    );
    assert_eq!(
        left["provenance"]["binding_sha256"],
        right["provenance"]["binding_sha256"]
    );
    assert_ne!(
        left["provenance"]["manifest_sha256"],
        right["provenance"]["manifest_sha256"]
    );
    for (name, bytes) in first.files().filter(|(name, _)| {
        name.contains("/internal/bridge/") || name.contains("/internal/descriptor/")
    }) {
        assert_eq!(bytes, fixture.file(&second, name), "{name}");
    }
}

#[test]
fn sg_layout_07_semantic_formatting_preserves_public_contract_surfaces() {
    let fixture = Fixture::new();
    fixture.prepare(SCHEMA, &["AdapterKind"]);
    let manifest = ResolvedManifest::load(&fixture.0.join("chigiri.toml")).unwrap();
    let rendered = render_contract(&manifest).unwrap();
    let marker: serde_json::Value =
        serde_json::from_slice(fixture.file(&rendered, GENERATED_MARKER_FILE)).unwrap();
    assert_eq!(marker["abi"], 8);
    assert!(marker["provenance"]["schema_sha256"].is_string());
    assert!(marker["provenance"]["binding_sha256"].is_string());
    for path in [
        "src/__chigiri/resolvers/objects/item.rs",
        "src/__chigiri/internal/bridge/objects/item.rs",
        "src/__chigiri/internal/descriptor/schema.rs",
    ] {
        assert!(!fixture.file(&rendered, path).is_empty(), "{path}");
    }
    fixture.write_rendered(&rendered);
    fixture.prepare_consumer(CONSUMER);
    fixture.cargo("consumer/Cargo.toml", &["test"], true);

    let old = "src/__chigiri/internal/descriptor/schema.rs";
    let new = "src/__chigiri/internal/layout/schema_descriptor.rs";
    for root in ["generated/contract", "private/contract"] {
        let root = fixture.0.join(root);
        let target = root.join(new);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::rename(root.join(old), &target).unwrap();
        let modules = root.join("src/__chigiri/modules.rs");
        let source = fs::read_to_string(&modules).unwrap();
        fs::write(
            &modules,
            source.replace(
                old.strip_prefix("src/__chigiri/").unwrap(),
                new.strip_prefix("src/__chigiri/").unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), fixture.file(&rendered, old));
    }
    for (path, bytes) in rendered.files().filter(|(path, _)| {
        path.starts_with("src/__chigiri/internal/bridge/")
            || path.starts_with("src/__chigiri/internal/dispatch/")
            || path.starts_with("src/__chigiri/resolvers/")
            || path.starts_with("src/__chigiri/types/")
            || *path == "src/__chigiri/schema_types.rs"
            || *path == "src/lib.rs"
    }) {
        assert_eq!(
            fs::read(fixture.0.join("generated/contract").join(path)).unwrap(),
            bytes,
            "{path}"
        );
    }
    fixture.cargo("consumer/Cargo.toml", &["test"], true);
    fixture.cargo("private/contract/Cargo.toml", &["check"], true);
}
