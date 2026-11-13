//! Verify schema/config conversion, readiness separation, leak prevention, exit codes, and direct output.

use super::harness::{BINARY, BLOG, CONFIG, Directory, wait};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
fn mr_17_18_19_same_binary_schema_and_closed_config_generate_identical_reference_offline() {
    let directory = Directory::new();
    fs::write(directory.0.join("blog.graphql"), BLOG).unwrap();
    fs::write(directory.0.join("mock.toml"), CONFIG).unwrap();
    let args = ["reference", "--schema", "blog.graphql", "--format", "json"];
    let sdl = directory.run(&args);
    assert!(
        sdl.status.success(),
        "{}",
        String::from_utf8_lossy(&sdl.stderr)
    );
    assert!(sdl.stderr.is_empty());
    let config = directory.run(&["reference", "--config", "mock.toml", "--format", "json"]);
    assert!(
        config.status.success(),
        "{}",
        String::from_utf8_lossy(&config.stderr)
    );
    let value: Value = serde_json::from_slice(&sdl.stdout).unwrap();
    let bundle: Value = serde_json::from_slice(&config.stdout).unwrap();
    // The producer normalizes embedded SDL tokens and source names. Its digest
    // identifies those adopted bytes rather than the original authoring file.
    let embedded = CONFIG.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        bundle["schema"]["digest"],
        embedded["storage"]["digest"].as_str().unwrap()
    );
    assert_ne!(value["schema"]["digest"], bundle["schema"]["digest"]);
    same_contract(&sdl.stdout, &config.stdout);
    assert_eq!(sdl.stdout, directory.run(&args).stdout);
    assert_eq!(value["readiness_checked"], false);
    assert_eq!(value["input_kind"], "schema");
    assert_eq!(value["config_executed"], false);
    assert!(value["mock_format"].is_null());
    assert_eq!(bundle["input_kind"], "bundle");
    assert_eq!(bundle["mock_format"], "chigiri-mock/v11");
    assert_eq!(bundle["config_executed"], false);
    assert_eq!(value["schema"]["models"].as_array().unwrap().len(), 2);
    let copied = directory.0.join("copied-mock");
    fs::copy(BINARY, &copied).unwrap();
    assert_eq!(fs::read(&copied).unwrap(), fs::read(BINARY).unwrap());
    fs::remove_file(directory.0.join("blog.graphql")).unwrap();
    let output = directory.run_command(
        Command::new(copied)
            .current_dir(&directory.0)
            .env_clear()
            .env("PATH", "")
            .args([
                "mock",
                "reference",
                "--config",
                "mock.toml",
                "--format",
                "json",
            ]),
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, config.stdout);
}

#[test]
fn rep_10_reference_text_and_json_share_new_scalar_vocabulary() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("scalar.graphql"),
        "scalar Token @representation(type: STRING, minBytes: 2) type Query{token:Token}",
    )
    .unwrap();
    let json = directory.run(&[
        "reference",
        "--schema",
        "scalar.graphql",
        "--format",
        "json",
    ]);
    assert!(json.status.success());
    let value: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["schema"]["vocabulary"], "chigiri-mock-directives/v3");
    let scalar = value["schema"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ty| ty["name"] == "Token")
        .unwrap();
    assert_eq!(scalar["scalar"]["representation"], "string");
    assert_eq!(scalar["scalar"]["constraints"]["min_bytes"], 2);
    let text = directory.run(&["reference", "all", "--schema", "scalar.graphql"]);
    assert!(text.status.success());
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("chigiri-mock-directives/v3")
    );
}

pub(super) fn same_contract(left: &[u8], right: &[u8]) {
    let mut left: Value = serde_json::from_slice(left).unwrap();
    let mut right: Value = serde_json::from_slice(right).unwrap();
    // Normalize configured-state differences before comparing the common reference contract.
    for doc in [&mut left, &mut right] {
        let digest = doc["schema"]["digest"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        doc["schema"].as_object_mut().unwrap().remove("digest");
        let configured = matches!(doc["input_kind"].as_str(), Some("project" | "bundle"));
        assert_eq!(doc["state_configured"], configured);
        assert_eq!(doc["events"]["configured"], configured);
        assert_eq!(doc["events"]["consumers"], serde_json::json!([]));
        doc["events"]["configured"] = Value::Bool(false);
        for id in ["ChigiriState", "ChigiriStatePatch"] {
            let row = doc["api_types"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["id"] == id)
                .unwrap();
            let generic =
                serde_json::json!({"kind":"map","value_type_id":"json","string_keys":true});
            assert_eq!(
                row["shape"],
                if configured {
                    serde_json::json!({"kind":"object","closed":true,"fields":[]})
                } else {
                    generic.clone()
                }
            );
            row["shape"] = generic;
        }
        for hook in doc["hooks"].as_array_mut().unwrap() {
            assert_eq!(
                hook["enabled"],
                if configured {
                    Value::Bool(false)
                } else {
                    Value::Null
                }
            );
            hook["enabled"] = Value::Null;
        }
    }
    for key in [
        "input_kind",
        "config_executed",
        "mock_format",
        "state_configured",
    ] {
        left.as_object_mut().unwrap().remove(key);
        right.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(left, right);
}

#[test]
fn mr_18_19_invalid_bodies_and_bad_seed_are_not_executed_or_disclosed() {
    let directory = Directory::new();
    let mut config = CONFIG.parse::<toml_edit::DocumentMut>().unwrap();
    config["server"]["listen"] = toml_edit::value("0.0.0.0:4000");
    config["server"]["allowed_origins"] = toml_edit::value(toml_edit::Array::from_iter(["*"]));
    config["seed"]["records_json"] = toml_edit::value("private-seed-invalid-json");
    for (_, resolver) in config["resolvers"].as_table_mut().unwrap().iter_mut() {
        resolver["luau"] = toml_edit::value("private-luau invalid ! require('private-module')");
    }
    fs::write(directory.0.join("private-config.toml"), config.to_string()).unwrap();
    for format in ["text", "json"] {
        let output = directory.run_command(
            directory
                .command()
                .env("PRIVATE_REFERENCE_SECRET", "private-environment-secret")
                .args([
                    "reference",
                    "--config",
                    "private-config.toml",
                    "--format",
                    format,
                ]),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let body = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "private-seed",
            "private-luau",
            "private-module",
            "private-environment-secret",
            "private-config.toml",
            directory.0.to_str().unwrap(),
        ] {
            assert!(!body.contains(secret), "{secret}");
        }
        assert!(body.contains("readiness_checked"));
    }
    let rejected = directory.run(&["serve", "--config", "private-config.toml"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty());
}

#[test]
fn mr_19_20_reference_validation_failure_has_no_body_and_separates_io_and_usage() {
    let directory = Directory::new();
    let legacy = include_str!("../../fixtures/mock-v9-ready.toml");
    let current_with_legacy_capability = CONFIG.replacen(
        "chigiri-mock-capabilities/v4",
        "chigiri-mock-capabilities/v2",
        1,
    );
    let legacy_with_current_capability = legacy.replacen(
        "chigiri-mock-capabilities/v2",
        "chigiri-mock-capabilities/v4",
        1,
    );
    assert_ne!(current_with_legacy_capability, CONFIG);
    assert_ne!(legacy_with_current_capability, legacy);
    let current_document = current_with_legacy_capability
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let legacy_document = legacy_with_current_capability
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    assert_eq!(
        current_document["format"].as_str(),
        Some("chigiri-mock/v11")
    );
    assert_eq!(
        current_document["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v2")
    );
    assert_eq!(legacy_document["format"].as_str(), Some("chigiri-mock/v9"));
    assert_eq!(
        legacy_document["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v4")
    );
    for source in [
        CONFIG.replace("chigiri-mock/v11", "chigiri-mock/v1"),
        current_with_legacy_capability,
        legacy_with_current_capability,
        format!("unknown=1\n{CONFIG}"),
        "malformed = [".into(),
    ] {
        assert_ne!(source, CONFIG);
        fs::write(directory.0.join("bad.toml"), source).unwrap();
        let output = directory.run(&["reference", "--config", "bad.toml", "--format", "json"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    fs::write(directory.0.join("bad.graphql"), "invalid SDL private-value").unwrap();
    let output = directory.run(&["reference", "--schema", "bad.graphql", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    for args in [
        vec!["reference", "--schema", "missing.graphql"],
        vec!["reference", "--config", "missing.toml"],
        vec!["reference", "--format", "yaml"],
        vec!["reference", "--config", "a", "--schema", "b"],
    ] {
        let output = directory.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    fs::write(directory.0.join("invalid-utf8"), [0xff]).unwrap();
    let output = directory.run(&["reference", "--schema", "invalid-utf8"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    fs::write(directory.0.join("mock.toml"), "invalid implicit file").unwrap();
    let output = directory.run(&["reference", "--format", "json"]);
    assert!(output.status.success());
    assert!(serde_json::from_slice::<Value>(&output.stdout).unwrap()["schema"].is_null());
}

#[test]
fn mr_21_22_pipe_is_plain_broken_pipe_succeeds_and_real_disk_failure_is_two() {
    let directory = Directory::new();
    let plain = directory.run(&["reference"]);
    let json = directory.run(&["reference", "--format", "json"]);
    for args in [vec!["reference"], vec!["reference", "--format", "text"]] {
        let output = directory.run_command(
            directory
                .command()
                .env("PAGER", "missing-pager | command")
                .args(args),
        );
        assert_eq!(output.stdout, plain.stdout);
        assert!(output.stderr.is_empty());
    }
    let mut child = directory
        .command()
        .args(["reference", "--format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(wait(child, Duration::from_secs(5)).success());
    assert_eq!(bytes, json.stdout);
    let mut child = directory
        .command()
        .args(["reference", "--format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(wait(child, Duration::from_secs(5)).success());
    // Linux uses a real device; macOS exercises the real I/O failure through the PTY test's file-size limit.
    #[cfg(target_os = "linux")]
    {
        let child = directory
            .command()
            .args(["reference", "--format", "json"])
            .stdout(
                std::fs::File::options()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert_eq!(wait(child, Duration::from_secs(5)).code(), Some(2));
    }
    let rejected = directory.run(&["reference", "--no-pager"]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
}
