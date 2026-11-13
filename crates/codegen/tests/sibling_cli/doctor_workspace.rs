//! Verifies workspace package selection and the scope of consumer features and
//! `--all-targets` with real Cargo invocations.

use std::fs;

use super::doctor_fixture::{CONSUMER, inputs, lock, prepare};
use super::fixture::report;

#[test]
fn sg_proc_02_resolves_target_packages_from_virtual_workspace_globs_and_exclusions() {
    let fixture = prepare("");
    let consumer = fixture.root.join("consumer");
    let main = consumer.join("app-main");
    fs::create_dir(&main).unwrap();
    fs::rename(consumer.join("src"), main.join("src")).unwrap();
    let manifest = fs::read_to_string(consumer.join("Cargo.toml"))
        .unwrap()
        .replace("[workspace]\n", "")
        .replace("../generated/contract", "../../generated/contract")
        .replace("../../../../crates/facade", "../../../../../crates/facade");
    fs::write(main.join("Cargo.toml"), manifest).unwrap();
    fs::write(
        consumer.join("Cargo.toml"),
        "[workspace]\nmembers=[\"app-*\"]\nexclude=[\"app-hidden\"]\nresolver=\"2\"\n",
    )
    .unwrap();
    fs::create_dir_all(consumer.join("app-hidden/src")).unwrap();
    fs::write(
        consumer.join("app-hidden/src/lib.rs"),
        "compile_error!(\"This package must remain excluded.\");\n",
    )
    .unwrap();
    fs::write(consumer.join("app-hidden/Cargo.toml"), "[package]\nname=\"sibling-doctor-consumer\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n").unwrap();
    lock(&fixture);
    let original = inputs(&fixture);
    let output = fixture.run("doctor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report(&output)
            .to_string()
            .contains("consumer/app-main/Cargo.toml")
    );
    assert_eq!(inputs(&fixture), original);
    fs::write(main.join("src/lib.rs"), "pub struct Types; impl ::contract::SchemaTypes for Types {} pub struct Query; impl ::contract::QueryResolver<(), Types, chigiri::FieldError> for Query {}\n").unwrap();
    let output = fixture.run("doctor");
    assert_eq!(output.status.code(), Some(1));
    let value = report(&output);
    let error = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["code"] == "E0046")
        .unwrap();
    assert!(
        error["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|label| label["path"] == "consumer/app-main/src/lib.rs"),
        "{value}"
    );
}

#[test]
fn sg_proc_02_dependency_feature_specifications_match_cargo_semantics() {
    let fixture = prepare("features=[\"dep/selected\"]");
    let dependency = fixture.root.join("feature-dependency");
    fs::create_dir_all(dependency.join("src")).unwrap();
    fs::write(dependency.join("Cargo.toml"), "[package]\nname=\"doctor-feature-dependency\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[features]\nselected=[]\n").unwrap();
    fs::write(
        dependency.join("src/lib.rs"),
        "#[cfg(feature=\"selected\")] pub const ENABLED: u8 = 1;\n",
    )
    .unwrap();
    let path = fixture.root.join("consumer/Cargo.toml");
    let mut manifest = fs::read_to_string(&path).unwrap();
    manifest
        .push_str("dep={package=\"doctor-feature-dependency\",path=\"../feature-dependency\"}\n");
    fs::write(path, manifest).unwrap();
    fs::write(
        fixture.root.join("consumer/src/lib.rs"),
        format!("{CONSUMER}\npub fn selected() -> u8 {{ dep::ENABLED }}\n"),
    )
    .unwrap();
    lock(&fixture);
    let output = fixture.run("doctor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report(&output)
            .to_string()
            .contains("--features dep/selected")
    );
}

#[test]
fn sg_proc_02_all_targets_checks_consumer_test_targets_by_default() {
    let fixture = prepare("");
    fs::create_dir_all(fixture.root.join("consumer/tests")).unwrap();
    fs::write(
        fixture.root.join("consumer/tests/required.rs"),
        "compile_error!(\"Diagnostic from the test target.\");\n",
    )
    .unwrap();
    let output = fixture.run("doctor");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        report(&output)
            .to_string()
            .contains("Diagnostic from the test target.")
    );
}
