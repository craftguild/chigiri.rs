//! Test pre-commit tool failures and input or cache changes during candidate validation.

use std::fs;

use crate::contract::check::fixture::Fixture;
use crate::filesystem::FsError;
use crate::{ResolvedManifest, render_contract, update_contract};

use super::tests::{configure_cache, live, prepared};
use super::transaction::{STAGE, Step};

#[test]
fn sg_fs_03_detects_manifest_and_sdl_changes_after_candidate_creation_and_preserves_live_output() {
    for path in ["chigiri.toml", "graphql/schema.graphql"] {
        let (fixture, mut transaction, old) = prepared();
        let error = transaction
            .install(&fixture.manifest, &mut |step| {
                if step == Step::Verify {
                    fs::write(fixture.root.join(path), b"changed during compile").unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        let _ = transaction.finish_failure(error);
        assert_eq!(live(&fixture), old);
        assert_eq!(transaction.parent.absent(STAGE), Ok(()));
    }
}

#[test]
fn input_failure_keeps_the_old_tree_and_clears_only_candidate_properties() {
    let fixture = Fixture::new();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    let old = live(&fixture);
    fs::write(fixture.root.join("graphql/schema.graphql"), b"type Query {").unwrap();
    assert!(update_contract(&fixture.manifest, crate::DiagnosticPolicy::default()).is_err());
    assert_eq!(live(&fixture), old);
    assert!(!fixture.output().parent().unwrap().join(STAGE).exists());
}

#[test]
fn cache_inside_sdk_rejects_cargo_check_and_preserves_the_old_tree() {
    if super::tests::isolated_cargo_config_test(
        "contract::write::input_tests::cache_inside_sdk_rejects_cargo_check_and_preserves_the_old_tree",
    ) {
        return;
    }
    let fixture = Fixture::new();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    let old = live(&fixture);
    fs::create_dir(fixture.root.join(".cargo")).unwrap();
    fs::write(
        fixture.root.join(".cargo/config.toml"),
        format!(
            "[build]\ntarget-dir={}\n[net]\noffline=true\n",
            toml_edit::Value::from(fixture.output().join("cache").to_str().unwrap())
        ),
    )
    .unwrap();
    let error = update_contract(&fixture.manifest, crate::DiagnosticPolicy::default()).unwrap_err();
    assert!(error.to_string().contains("SF4023"));
    assert_eq!(live(&fixture), old);
    assert!(!fixture.output().parent().unwrap().join(STAGE).exists());
}

#[test]
fn sg_fs_03_real_cargo_compile_failure_preserves_old_output_and_manifest_bytes() {
    let fixture = Fixture::new();
    fixture.write(&render_contract(&fixture.manifest).unwrap());
    let old = live(&fixture);
    configure_cache(&fixture);
    fs::create_dir_all(fixture.root.join("mock/crates/facade/src")).unwrap();
    fs::write(
        fixture.root.join("mock/crates/facade/Cargo.toml"),
        "[package]\nname=\"chigiri\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        fixture.root.join("mock/crates/facade/src/lib.rs"),
        "pub mod core {}\n",
    )
    .unwrap();
    let source = fs::read_to_string(fixture.root.join("chigiri.toml"))
        .unwrap()
        .replace("root=\"../../..\"", "root=\"mock\"");
    fs::write(fixture.root.join("chigiri.toml"), &source).unwrap();
    let manifest = ResolvedManifest::load(&fixture.root.join("chigiri.toml")).unwrap();
    let error = update_contract(&manifest, crate::DiagnosticPolicy::default()).unwrap_err();
    assert!(error.to_string().contains("SF4025"), "{error}");
    assert_eq!(live(&fixture), old);
    assert_eq!(
        fs::read_to_string(fixture.root.join("chigiri.toml")).unwrap(),
        source
    );
    assert!(!fixture.output().parent().unwrap().join(STAGE).exists());
}

#[test]
fn sg_fs_02_rejects_output_nested_inside_another_schema_before_parent_creation() {
    let fixture = Fixture::new();
    let source = fs::read_to_string(fixture.root.join("chigiri.toml"))
        .unwrap()
        .replace("generated/graphql-contract", "GraphQL/contract");
    fs::write(fixture.root.join("chigiri.toml"), source).unwrap();
    let manifest = ResolvedManifest::load(&fixture.root.join("chigiri.toml")).unwrap();
    // In macOS, the default volume is that GraphQL and graphql refer to the same inode.
    // On a case-sensitive host this is a distinct directory, so exercise the condition there.
    if manifest.root.relative("GraphQL").is_ok() {
        assert!(
            super::transaction::Transaction::begin(&manifest, super::Operation::Generate).is_err()
        );
        assert!(!fixture.root.join("graphql/contract").exists());
    } else {
        assert!(matches!(
            manifest.root.relative("GraphQL"),
            Err(FsError::Missing)
        ));
    }
}
