//! Test manifest-only doctor flows with real consumers, Cargo, and failures.

#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "../../codegen/tests/sibling_cli/doctor_inputs.rs"]
mod consumer_inputs;
#[path = "../../codegen/tests/sibling_cli/doctor_fixture.rs"]
mod doctor_fixture;
#[path = "../../codegen/tests/sibling_cli/doctor_failures.rs"]
mod failures;
#[path = "../../codegen/tests/sibling_cli/fixture.rs"]
mod fixture;
#[path = "../../codegen/tests/sibling_cli/doctor_output.rs"]
mod output;
#[path = "../../codegen/tests/sibling_cli/doctor_traits.rs"]
mod traits;
#[path = "../../codegen/tests/sibling_cli/doctor_workspace.rs"]
mod workspace;

use std::fs;

use doctor_fixture::{CONSUMER, inputs, lock, prepare, tool_log};
use fixture::report;

#[test]
fn sg_proc_02_doctor_compiles_a_real_consumer_alias_with_locked_all_target_options() {
    let fixture = prepare("");
    let original = inputs(&fixture);
    let sdk = fixture.snapshot();
    let output = fixture.run("doctor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = report(&output);
    let note = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["origin"] == "cargo")
        .unwrap();
    let argv = note["notes"][0].as_str().unwrap();
    for selected in [
        "--locked",
        "--package sibling-doctor-consumer",
        "--all-targets",
        "--manifest-path consumer/Cargo.toml",
    ] {
        assert!(argv.contains(selected), "{argv}");
    }
    assert_eq!(inputs(&fixture), original);
    assert_eq!(fixture.snapshot(), sdk);
}

#[test]
fn sg_proc_02_real_consumer_missing_resolver_returns_the_rustc_error_as_a_diagnostic() {
    let fixture = prepare("");
    let begin = CONSUMER.find("    async fn value").unwrap();
    let end = CONSUMER[begin..].find("\n}").unwrap() + begin;
    let mut source = CONSUMER.to_owned();
    source.replace_range(begin..end, "");
    fs::write(fixture.root.join("consumer/src/lib.rs"), source).unwrap();
    let original = inputs(&fixture);
    let output = fixture.run("doctor");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = report(&output);
    let rustc = value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["code"] == "E0046")
        .unwrap();
    assert_eq!(rustc["origin"], "rustc");
    assert_eq!(rustc["coordinate"], "Query.value");
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["code"] == "SF4018" && record["coordinate"] == "Query.value"),
        "{value}"
    );
    assert!(
        rustc["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|label| label["path"] == "consumer/src/lib.rs"),
        "{value}"
    );
    assert_eq!(inputs(&fixture), original);
}

#[test]
fn sg_proc_02_static_drift_does_not_start_cargo() {
    let fixture = prepare("");
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { other: String }",
    )
    .unwrap();
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        report(&output)["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["code"] == "SF4009")
    );
    assert!(!log.exists());
}

#[test]
fn sg_proc_02_rejects_dependencies_on_another_generated_crate_before_cargo_check() {
    let fixture = prepare("");
    let other = fixture.root.join("generated/other");
    for (path, bytes) in fixture.snapshot() {
        let path = other.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let path = fixture.root.join("consumer/Cargo.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("../generated/contract", "../generated/other"),
    )
    .unwrap();
    lock(&fixture);
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        report(&output)["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["code"] == "SF4015")
    );
    assert_eq!(fs::read_to_string(log).unwrap(), "metadata\nmetadata\n");
}

#[test]
fn sg_proc_02_passes_manifest_features_and_all_targets_to_real_cargo() {
    let fixture = prepare("features=[\"selected\"]\nno_default_features=true\nall_targets=false");
    let mut source = CONSUMER.to_owned();
    source.push_str("\n#[cfg(feature=\"default-marker\")]\ncompile_error!(\"default feature must be disabled\");\n#[cfg(not(feature=\"selected\"))]\ncompile_error!(\"selected feature must be enabled\");\n#[cfg(test)]\ncompile_error!(\"all-targets must not compile test configuration\");\n");
    fs::write(fixture.root.join("consumer/src/lib.rs"), source).unwrap();
    let output = fixture.run("doctor");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = report(&output);
    let text = value.to_string();
    assert!(text.contains("--no-default-features"));
    assert!(text.contains("--features selected"));
    assert!(!text.contains("--all-targets"));
}
