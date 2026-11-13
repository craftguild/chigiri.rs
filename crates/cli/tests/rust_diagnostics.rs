//! Verifies the external CLI contract shared by structured diagnostics and a
//! real consumer doctor run.

#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "../../codegen/tests/sibling_cli/doctor_fixture.rs"]
mod doctor_fixture;
#[path = "../../codegen/tests/sibling_cli/fixture.rs"]
mod fixture;
#[path = "../../codegen/tests/dx_diagnostics/tools.rs"]
mod tools;
#[path = "../../codegen/tests/dx_diagnostics/workspace.rs"]
mod workspace;

use std::collections::BTreeSet;
use std::fs;

use doctor_fixture::{inputs, prepare, tool_log};
use fixture::{Fixture, report};

#[test]
fn exposes_public_models_and_human_and_json_renderers() {
    let label = chigiri_codegen::DiagnosticLabel::new(
        "graphql/schema.graphql",
        3..7,
        2,
        4,
        "invalid field",
    );
    let report = chigiri_codegen::DiagnosticReport::new(
        vec![
            chigiri_codegen::CodegenDiagnostic::error("SF2001", "unknown field")
                .with_primary(label)
                .with_help("Define the field.")
                .with_note("schema validation"),
        ],
        true,
    );
    let error = chigiri_codegen::CodegenError::from_report(report);
    assert_eq!(error.report().diagnostics().len(), 1);
    assert!(error.report().truncated());
    let human = chigiri_codegen::render_diagnostics(
        error.report(),
        chigiri_codegen::DiagnosticFormat::Human,
    );
    assert!(human.starts_with("error[SF2001]"));
    assert!(human.find("help:").unwrap() < human.find("note: schema").unwrap());
    let json = chigiri_codegen::render_diagnostics(
        error.report(),
        chigiri_codegen::DiagnosticFormat::Json,
    );
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema_version"], "chigiri/diagnostics-v2");
    assert_eq!(value["truncated"], true);
    assert_eq!(value["diagnostics"][0]["origin"], "chigiri");
    assert_eq!(value["diagnostics"][0]["labels"][0]["start"], 3);
    assert!(
        value["diagnostics"][0]["labels"][0]
            .get("excerpt")
            .is_none()
    );
    assert_eq!(value["diagnostics"][1]["code"], "SF4007");
}

#[test]
fn diag_03_commands_and_retired_names_report_one_json_diagnostic_on_stderr() {
    let fixture = Fixture::new();
    for command in [
        "init", "codegen", "update", "check", "doctor", "generate", "snapshot", "diff", "scaffold",
    ] {
        let output = fixture.execute(fixture.command(command).arg("--unknown-option"));
        assert_eq!(output.status.code(), Some(2));
        let value = report(&output);
        assert_eq!(value["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(value["diagnostics"][0]["origin"], "chigiri");
        assert_eq!(value["diagnostics"][0]["code"], "SF4001");
        assert!(value["diagnostics"][0].get("formatVersion").is_none());
    }
}

#[test]
fn diag_04_07_13_14_static_after_successfully_fixing_cargo_and_side_effects() {
    let fixture = prepare("");
    let before = inputs(&fixture);
    let sdk = fixture.snapshot();
    let paths = tool_log(&fixture);
    let log = fixture.root.join("cargo.log");
    let output = fixture.execute(
        fixture
            .command("doctor")
            .env("PATH", paths)
            .env("CHIGIRI_TEST_CARGO_LOG", &log)
            .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "metadata\nmetadata\ncheck\n"
    );
    assert_eq!(inputs(&fixture), before);
    assert_eq!(fixture.snapshot(), sdk);
    let value = report(&output);
    let records = value["diagnostics"].as_array().unwrap();
    let note = records
        .iter()
        .find(|record| record["origin"] == "cargo")
        .unwrap();
    let argv = note["notes"][0].as_str().unwrap();
    for token in [
        "--locked",
        "--package sibling-doctor-consumer",
        "--message-format=json",
        "--all-targets",
        "--manifest-path consumer/Cargo.toml",
    ] {
        assert!(argv.contains(token), "{argv}");
    }
    for record in records {
        assert_eq!(
            record
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            [
                "code",
                "coordinate",
                "help",
                "labels",
                "message",
                "notes",
                "origin",
                "severity"
            ]
            .into_iter()
            .collect()
        );
        assert!(!record.to_string().contains(fixture.root.to_str().unwrap()));
    }
}

#[test]
fn diag_05_drift_exits_one_without_starting_cargo() {
    let fixture = prepare("");
    let path = fixture.output().join("src/lib.rs");
    fs::write(
        &path,
        fs::read_to_string(&path).unwrap() + "// drift for testing.\n",
    )
    .unwrap();
    let before = inputs(&fixture);
    let sdk = fixture.snapshot();
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
    let value = report(&output).to_string();
    assert!(
        value.contains("SF4016")
            && value.contains("SF4020")
            && value.contains("changed: src/lib.rs")
            && value.contains("chigiri rust --manifest 'selected.toml' codegen")
            && value.contains("different name"),
        "{value}"
    );
    assert!(!log.exists());
    assert_eq!(inputs(&fixture), before);
    assert_eq!(fixture.snapshot(), sdk);
}
