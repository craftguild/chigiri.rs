//! Uses the CLI to verify changes in user inputs and the running tool.

use std::fs;
use std::process::Command;

use super::doctor_fixture::{CONSUMER, inputs, lock, prepare, tool_log};
use super::fixture::report;

#[test]
fn sg_proc_02_accepts_sdk_dev_aliases_for_all_targets_and_rejects_build_aliases() {
    for (kind, all_targets, success) in [
        ("dev", true, true),
        ("dev", false, false),
        ("build", true, false),
    ] {
        let fixture = prepare(&format!("all_targets={all_targets}"));
        let path = fixture.root.join("consumer/Cargo.toml");
        let cargo = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            cargo.replace("[dependencies]", &format!("[{kind}-dependencies]")),
        )
        .unwrap();
        fs::write(
            fixture.root.join("consumer/src/lib.rs"),
            format!("#![cfg(test)]\n{CONSUMER}"),
        )
        .unwrap();
        lock(&fixture);
        let original = inputs(&fixture);
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
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = report(&output);
        if success {
            assert_eq!(
                fs::read_to_string(log).unwrap(),
                "metadata\nmetadata\ncheck\n"
            );
        } else {
            assert_eq!(output.status.code(), Some(1));
            assert!(value.to_string().contains("SF4015"), "{value}");
            assert_eq!(fs::read_to_string(log).unwrap(), "metadata\nmetadata\n");
        }
        assert_eq!(inputs(&fixture), original);
        assert_eq!(fixture.snapshot(), sdk);
    }
}

#[test]
fn sg_cli_06_unknown_dependency_feature_is_reported_as_input_failure() {
    let fixture = prepare("features=[\"contract/absent\"]");
    let raw = fixture.execute(
        Command::new(env!("CARGO"))
            .current_dir(&fixture.root)
            .args([
                "metadata",
                "--format-version=1",
                "--locked",
                "--offline",
                "--features",
                "contract/absent",
                "--manifest-path",
            ])
            .arg(fixture.root.join("consumer/Cargo.toml")),
    );
    assert!(!raw.status.success());
    let output = fixture.run("doctor");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}\n{}",
        String::from_utf8_lossy(&raw.stderr),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report(&output).to_string().contains("SF4015"));
}

#[test]
fn sg_fs_02_rejects_consumer_manifest_and_parent_symlinks_before_cargo() {
    for parent in [false, true] {
        let fixture = prepare("");
        let (original, moved) = if parent {
            (
                fixture.root.join("consumer"),
                fixture.root.join("real-consumer"),
            )
        } else {
            (
                fixture.root.join("consumer/Cargo.toml"),
                fixture.root.join("consumer/real.toml"),
            )
        };
        fs::rename(&original, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &original).unwrap();
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
        let value = report(&output);
        assert!(value.to_string().contains("SF4015"), "{value}");
        assert!(!log.exists());
    }
}

#[test]
fn sg_proc_02_inputs_changed_during_cargo_are_not_reported_as_success() {
    for relative in [
        "graphql/schema.graphql",
        "consumer/Cargo.toml",
        "generated/contract/src/lib.rs",
    ] {
        let fixture = prepare("");
        let path = fixture.root.join(relative);
        let old = fs::read(&path).unwrap();
        let paths = fixture.wrapper(
            "cargo",
            concat!(
                "#!/bin/sh\n# Inject an input change after the fixture is captured.\n",
                "\"$CHIGIRI_TEST_REAL_CARGO\" \"$@\"\nstatus=$?\n",
                "if [ \"$1\" = check ]; then printf '\\n' >> \"$CHIGIRI_TEST_MUTATED_INPUT\"; fi\n",
                "exit \"$status\"\n"
            ),
        );
        let output = fixture.execute(
            fixture
                .command("doctor")
                .env("PATH", paths)
                .env("CHIGIRI_TEST_REAL_CARGO", env!("CARGO"))
                .env("CHIGIRI_TEST_MUTATED_INPUT", &path),
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !report(&output)["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(path).unwrap(), [old.as_slice(), b"\n"].concat());
    }
}
