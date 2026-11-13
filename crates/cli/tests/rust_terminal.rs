//! Test unified-binary Rust help/init boundaries and fail-closed global limits.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chigiri-terminal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn cli(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_chigiri"))
            .arg("rust")
            .args(arguments)
            .current_dir(&self.0)
            .env("PATH", "/chigiri-no-tools")
            .env("GIT_DIR", "/chigiri-no-git")
            .env("CHIGIRI_MANIFEST", "ignored.toml")
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sg_cli_01_help_succeeds_at_all_levels_offline_without_a_manifest() {
    let fixture = Fixture::new();
    for command in [
        vec![],
        vec!["init"],
        vec!["codegen"],
        vec!["update"],
        vec!["check"],
        vec!["doctor"],
    ] {
        let mut args = vec!["--manifest", "missing.toml", "--format", "json"];
        args.extend(command);
        args.extend(["--deny-warnings", "--help"]);
        let output = fixture.cli(&args);
        assert!(output.status.success(), "{args:?}");
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Usage: chigiri rust"));
        assert!(help.contains("--manifest <PATH>"));
        assert!(!help.contains("--schema-dir"));
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn sg_cli_02_init_creates_manifest_once_and_ignores_environment_overrides() {
    let fixture = Fixture::new();
    let output = fixture.cli(&["init", "--format", "json", "--deny-warnings"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let manifest = fixture.0.join("chigiri.toml");
    assert_eq!(
        fs::read_to_string(&manifest).unwrap(),
        chigiri_codegen::MANIFEST_TEMPLATE
    );
    let output = fixture.cli(&["--format", "json", "init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    serde_json::from_slice::<serde_json::Value>(&output.stderr).unwrap();
    assert_eq!(
        fs::read_to_string(&manifest).unwrap(),
        chigiri_codegen::MANIFEST_TEMPLATE
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    fs::create_dir(fixture.0.join("nested")).unwrap();
    let output = fixture.cli(&["--manifest", "nested/chosen.toml", "init"]);
    assert!(output.status.success());
    assert_eq!(fs::read_dir(fixture.0.join("nested")).unwrap().count(), 1);
}

#[test]
fn sg_cli_04_help_and_init_reject_unknown_duplicate_and_legacy_options_before_side_effects() {
    let fixture = Fixture::new();
    for arguments in [
        vec!["init", "--config", "secret"],
        vec!["init", "--output-dir", "generated"],
        vec!["init", "--manifest", "first", "--manifest", "second"],
        vec!["init", "--deny-warnings", "--deny-warnings"],
        vec!["init", "--format", "json", "--format", "human"],
        vec!["init", "unexpected"],
        vec!["--help", "--version"],
        vec!["--help", "--help"],
        vec!["--help", "--manifest"],
        vec!["schema", "--help"],
        vec!["generate", "--help"],
        vec!["snapshot", "--help"],
        vec!["diff", "--help"],
        vec!["scaffold", "wrong", "--help"],
        vec!["doctor", "--features", "secret", "--help"],
        vec!["--format", "json", "init", "--format", "human"],
        vec!["--manifest", "first", "init", "--manifest", "second"],
        vec!["--deny-warnings", "init", "--deny-warnings"],
        vec!["--help", "init", "--help"],
    ] {
        let output = fixture.cli(&arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}
