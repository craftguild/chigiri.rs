//! Verifies create, update, check, and non-destructive diagnostic behavior through
//! the manifest-only CLI binary.

#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "../../codegen/tests/sibling_cli/fixture.rs"]
mod fixture;
#[path = "../../codegen/tests/sibling_cli/layout.rs"]
mod layout;
#[path = "../../codegen/tests/sibling_cli/ownership.rs"]
mod ownership;
#[path = "../../codegen/tests/sibling_cli/term.rs"]
mod term;

use std::fs;

use chigiri_codegen::GENERATED_MARKER_FILE;
use fixture::{Fixture, report};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

#[test]
fn sg_cli_06_runs_generate_update_and_check_with_the_selected_manifest() {
    let fixture = Fixture::new();
    let missing = fixture.run("check");
    assert_eq!(missing.status.code(), Some(1));
    let missing = report(&missing);
    assert_eq!(missing["diagnostics"][0]["code"], "SF4020");
    assert!(
        missing["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("generated crate I/O: A required file or directory is missing.")
    );
    assert!(!fixture.output().exists());
    fixture.success("codegen");
    assert!(!fixture.root.join("chigiri.toml").exists());
    assert!(!fixture.output().join("Cargo.lock").exists());
    assert!(!fixture.output().join("target").exists());
    let initial = fixture.snapshot();
    let manifest = fs::read(fixture.root.join("selected.toml")).unwrap();
    assert_eq!(fixture.run("codegen").status.code(), Some(1));
    assert_eq!(fixture.snapshot(), initial);
    fixture.success("check");
    fs::write(fixture.root.join("consumer.rs"), "modified source").unwrap();
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { value: String extra: Int }",
    )
    .unwrap();
    let output = fixture.run("check");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report(&output)["diagnostics"][0]["code"], "SF4009");
    assert_eq!(fixture.snapshot(), initial);
    fixture.success("update");
    assert_ne!(fixture.snapshot(), initial);
    fixture.success("check");
    assert_eq!(
        fs::read(fixture.root.join("selected.toml")).unwrap(),
        manifest
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("consumer.rs")).unwrap(),
        "modified source"
    );
    assert!(!fixture.root.join("generated/.chigiri-transaction").exists());
    assert!(!fixture.root.join("generated/.chigiri-recovery").exists());
}

#[test]
fn sg_cli_06_deny_warnings_fails_before_commit_while_default_policy_keeps_warnings() {
    let fixture = Fixture::new();
    fixture.success("codegen");
    let initial = fixture.snapshot();
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { extra: Int value: String }",
    )
    .unwrap();
    let paths = fixture.wrapper(
        "cargo",
        include_str!("../../codegen/tests/sibling_cli/warning-cargo.sh"),
    );
    for (deny, expected) in [(true, 1), (false, 0)] {
        let output = fixture.execute(
            fixture
                .command_with_policy("update", deny)
                .env("PATH", &paths)
                .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO")),
        );
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = report(&output);
        assert_eq!(value["diagnostics"][0]["origin"], "rustc");
        assert_eq!(value["diagnostics"][0]["code"], "fixture_warning");
        assert_eq!(value["diagnostics"][0]["severity"], "warning");
        if deny {
            assert_eq!(value["diagnostics"][1]["code"], "SF4028");
            assert_eq!(fixture.snapshot(), initial);
        } else {
            assert_ne!(fixture.snapshot(), initial);
        }
        assert!(!fixture.root.join("generated/.chigiri-transaction").exists());
    }
}

#[test]
fn sg_fs_01_real_cli_rejects_empty_directories_and_unknown_files_for_all_operations() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.output()).unwrap();
    for command in ["codegen", "update"] {
        assert_eq!(fixture.run(command).status.code(), Some(1));
        assert_eq!(fs::read_dir(fixture.output()).unwrap().count(), 0);
    }
    fs::write(
        fixture.output().join("handwritten.rs"),
        "generated output should preserve bytes",
    )
    .unwrap();
    let initial = fixture.snapshot();
    for command in ["codegen", "update"] {
        assert_eq!(fixture.run(command).status.code(), Some(1));
        assert_eq!(fixture.snapshot(), initial);
    }
}

#[test]
fn sg_cli_04_legacy_overrides_and_environment_values_defer_to_manifest_authority() {
    let fixture = Fixture::new();
    for command in ["codegen", "update", "check"] {
        for flag in [
            "--schema-dir",
            "--config",
            "--output-dir",
            "--diagnostic-format",
            "--features",
        ] {
            let output = fixture.execute(fixture.command(command).args([flag, "secret-value"]));
            assert_eq!(output.status.code(), Some(2), "{command}/{flag}");
            let value = report(&output);
            assert_eq!(value["diagnostics"][0]["code"], "SF4001");
            assert!(!value.to_string().contains("secret-value"));
        }
    }
    let output = fixture.execute(
        std::process::Command::new(env!("CARGO_BIN_EXE_chigiri"))
            .current_dir(&fixture.root)
            .args(["rust", "codegen", "--format", "json"])
            .env("CHIGIRI_MANIFEST", fixture.root.join("selected.toml")),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(!fixture.output().exists());
}

#[test]
fn sg_cli_07_manifest_and_tool_failures_form_one_redacted_report() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.root.join("selected.toml")).unwrap();
    fs::write(fixture.root.join("selected.toml"), "schema_version = 'https://user:private-password@example.org'\nsecret = 'private-password'\ninvalid = [\n").unwrap();
    let output = fixture.run("codegen");
    assert_eq!(output.status.code(), Some(1));
    let value = report(&output);
    assert_eq!(value["diagnostics"][0]["code"], "SF4011");
    for secret in [
        "private-password",
        "example.org",
        fixture.root.to_str().unwrap(),
    ] {
        assert!(!value.to_string().contains(secret));
    }
    fs::write(fixture.root.join("selected.toml"), &original).unwrap();
    let output = fixture.execute(fixture.command("codegen").env("PATH", "/chigiri-no-tools"));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(report(&output)["diagnostics"][0]["code"], "SF4008");
    assert!(!fixture.output().exists());
    assert!(!fixture.root.join("generated/.chigiri-transaction").exists());
    assert_eq!(
        fs::read(fixture.root.join("selected.toml")).unwrap(),
        original
    );
}

#[test]
fn sg_info_04_real_cli_does_not_update_different_records_and_reports_notice_without_cargo() {
    let fixture = Fixture::new();
    fixture.success("codegen");
    // Replay recorded provenance under a different profile, then compare Rust, marker, and inventory bytes.
    let marker_path = fixture.output().join(GENERATED_MARKER_FILE);
    let marker_source = fs::read_to_string(&marker_path).unwrap();
    let marker: Value = serde_json::from_str(&marker_source).unwrap();
    let old = marker["generator"]["build"]["profile"].as_str().unwrap();
    let new = if old == "release" { "debug" } else { "release" };
    let source_path = "src/__chigiri/generator_info.rs";
    let source = fs::read_to_string(fixture.output().join(source_path)).unwrap();
    let old_size = source.len();
    let old_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
    let source = source.replace(
        &format!("profile: ::std::borrow::Cow::Borrowed({old:?})"),
        &format!("profile: ::std::borrow::Cow::Borrowed({new:?})"),
    );
    fs::write(fixture.output().join(source_path), &source).unwrap();
    let start = marker_source
        .find(&format!("\"path\": {source_path:?}"))
        .unwrap();
    let end = start + marker_source[start..].find('}').unwrap();
    let replacement = marker_source[start..end]
        .replace(
            &old_hash,
            &format!("{:x}", Sha256::digest(source.as_bytes())),
        )
        .replace(
            &format!("\"bytes\": {old_size}"),
            &format!("\"bytes\": {}", source.len()),
        );
    let mut marker_source = marker_source;
    marker_source.replace_range(start..end, &replacement);
    let marker_source = marker_source.replace(
        &format!("\"profile\": {old:?}"),
        &format!("\"profile\": {new:?}"),
    );
    fs::write(&marker_path, marker_source).unwrap();
    let initial = fixture.snapshot();
    let paths = fixture.wrapper(
        "cargo",
        "#!/bin/sh\n# Fail if check starts Cargo for this fixture.\nprintf called > cargo-called\nexit 97\n",
    );
    let output = fixture.execute(fixture.command("check").env("PATH", paths));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = report(&output);
    assert_eq!(value["diagnostics"][0]["code"], "SF4027");
    assert_eq!(value["diagnostics"][0]["severity"], "note");
    let notes = value["diagnostics"][0]["notes"].as_array().unwrap();
    assert!(notes[0].as_str().unwrap().starts_with("recorded:"));
    assert!(notes[1].as_str().unwrap().starts_with("current:"));
    assert_eq!(fixture.snapshot(), initial);
    assert!(!fixture.root.join("cargo-called").exists());
    fs::write(fixture.output().join("src/unowned.rs"), "source, unknown.").unwrap();
    let changed = fixture.snapshot();
    assert_eq!(fixture.run("check").status.code(), Some(1));
    assert_eq!(fixture.snapshot(), changed);
}
