//! Verifies ADR-0007 ENUM-01 through ENUM-06 at the manifest and independent-SDK consumer boundary.

use std::fs;

#[path = "../../codegen/tests/support/cargo.rs"]
mod cargo;
#[path = "../../codegen/tests/vnext_enum_binding/fixture.rs"]
mod fixture;
#[path = "../../codegen/tests/vnext_enum_binding/source.rs"]
mod source;

use fixture::{Fixture, config};
use source::{fingerprint, generated_files, joined};

#[test]
fn enum_01_configured_and_owned_enums_generate_abi8_bindings() {
    let temporary = Fixture::new("owned");
    let config = config().split("[bindings]").next().unwrap().to_owned();
    fs::write(temporary.join("chigiri.toml"), config).unwrap();
    temporary.success("codegen");
    temporary.success("check");
    let files = generated_files(&temporary.join("generated"));
    let root = &files["src/lib.rs"];
    let private_root = &files["src/__chigiri/mod.rs"];
    let types = joined(&files, "src/__chigiri/types/");
    let resolvers = joined(&files, "src/__chigiri/resolvers/");

    assert!(root.contains("chigiri::GENERATED_ABI_VERSION == 8"));
    assert!(!root.contains("#![allow("));
    assert!(private_root.contains("schema content SHA-256:"));
    assert!(private_root.contains("enum binding config: chigiri.toml"));
    let manifest =
        chigiri_codegen::ResolvedManifest::load(&temporary.join("chigiri.toml")).unwrap();
    assert!(manifest.manifest().domain_enums().is_empty());
    assert!(types.contains("pub enum ProtocolKind"));
    assert!(!types.contains("ProtocolKindBinding"));
    assert!(resolvers.contains("std::result::Result<super::ProtocolKind, E>"));
    assert!(!resolvers.contains("T::ProtocolKind"));
}

#[test]
fn enum_02_05_generic_path_bindings_compile_and_mapping_failures_are_masked() {
    let temporary = Fixture::new("runtime");
    temporary.success("codegen");
    let files = generated_files(&temporary.join("generated"));
    let types = joined(&files, "src/__chigiri/types/");
    let bridge = joined(&files, "src/__chigiri/internal/bridge/");
    let schema_types = &files["src/__chigiri/schema_types.rs"];
    assert!(schema_types.contains("fn protocol_kind_from_graphql("));
    assert!(schema_types.contains("fn protocol_kind_to_graphql("));
    assert!(types.contains("pub struct NestedInput<T: super::SchemaTypes>"));
    assert!(types.contains("pub struct FilterInput<T: super::SchemaTypes>"));
    assert!(types.contains("pub enum ChoiceInput<T: super::SchemaTypes>"));
    assert!(types.contains("pub enum OwnedKind"));
    assert!(bridge.matches("__chigiri_input_protocol_kind::<T>").count() >= 2);
    temporary.test_consumer();
    temporary.success("check");
}

#[test]
fn enum_03_invalid_config_shape_preserves_existing_output_with_sf4011() {
    assert_config_errors(
        "SF4011",
        &[
            "schema_version =\n".to_owned(),
            format!("unknown = true\n{}", config()),
            config().replace("[schema]\ndirectory", "schema.directory"),
        ],
    );
}

#[test]
fn enum_04_invalid_binding_name_preserves_existing_output_with_sf4012() {
    let cases = [
        "\"ProtocolKind\", \"ProtocolKind\"",
        "\"Missing\"",
        "\"Query\"",
    ]
    .map(|names| config().replace("[\"ProtocolKind\"]", &format!("[{names}]")));
    assert_config_errors("SF4012", &cases);
}

fn assert_config_errors(code: &str, cases: &[String]) {
    for (index, config) in cases.iter().enumerate() {
        let temporary = Fixture::new(&format!("invalid-{index}"));
        temporary.success("codegen");
        let before = generated_files(&temporary.join("generated"));
        fs::write(temporary.join("chigiri.toml"), config).unwrap();
        let output = temporary.cli("update");
        assert_eq!(output.status.code(), Some(1));
        let report = report(&output);
        let diagnostic = &report["diagnostics"][0];
        assert_eq!(diagnostic["code"], code, "{report}");
        let label = &diagnostic["labels"][0];
        assert_eq!(label["path"], "chigiri.toml", "{report}");
        assert!(label["line"].as_u64().unwrap() > 0);
        assert!(label["column"].as_u64().unwrap() > 0);
        assert!(label["start"].as_u64().unwrap() <= label["end"].as_u64().unwrap());
        assert_eq!(generated_files(&temporary.join("generated")), before);
    }
}

#[test]
fn enum_06_comment_changes_preserve_config_digest_but_change_descriptor_drift() {
    let temporary = Fixture::new("drift");
    fs::write(
        temporary.join("chigiri.toml"),
        format!("# First comment\n{}", config()),
    )
    .unwrap();
    temporary.success("codegen");
    let first = generated_files(&temporary.join("generated"));
    fs::write(
        temporary.join("chigiri.toml"),
        format!("# After the change comment \n {}", config()),
    )
    .unwrap();
    let output = temporary.cli("check");
    assert_eq!(output.status.code(), Some(1));
    let report = report(&output);
    assert_eq!(report["diagnostics"][0]["code"], "SF4009");
    assert!(
        report["diagnostics"][0]["help"]
            .as_array()
            .unwrap()
            .iter()
            .any(|help| help.as_str().unwrap().contains("chigiri rust update"))
    );
    assert_eq!(generated_files(&temporary.join("generated")), first);
    temporary.success("update");
    temporary.success("check");
    let second = generated_files(&temporary.join("generated"));

    assert_eq!(
        joined(&first, "src/__chigiri/internal/descriptor/"),
        joined(&second, "src/__chigiri/internal/descriptor/")
    );
    assert_eq!(
        fingerprint(&first, "fn schema_fingerprint"),
        fingerprint(&second, "fn schema_fingerprint")
    );
    assert_eq!(
        fingerprint(&first, "fn resolver_abi_fingerprint"),
        fingerprint(&second, "fn resolver_abi_fingerprint")
    );
    assert_ne!(
        first["src/__chigiri/mod.rs"],
        second["src/__chigiri/mod.rs"]
    );
    let first: serde_json::Value = serde_json::from_str(&first["chigiri-generated.json"]).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(&second["chigiri-generated.json"]).unwrap();
    assert_ne!(
        first["provenance"]["manifest_sha256"],
        second["provenance"]["manifest_sha256"]
    );
    assert_eq!(
        first["provenance"]["schema_sha256"],
        second["provenance"]["schema_sha256"]
    );
    assert_eq!(
        first["provenance"]["binding_sha256"],
        second["provenance"]["binding_sha256"]
    );
}

fn report(output: &chigiri_codegen::ProcessOutput) -> serde_json::Value {
    assert!(output.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(report["schema_version"], "chigiri/diagnostics-v2");
    assert_eq!(report["truncated"], false);
    report
}
