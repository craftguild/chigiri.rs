//! Copies the large SDL and handwritten consumer into an isolated root while
//! preserving their relative layout.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture(PathBuf);

pub(super) fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/vnext-large-product")
}

impl Fixture {
    pub(super) fn new(binding: bool) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sibling-generator")
            .join(format!(
                "large-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.join("graphql")).unwrap();
        let value = Self(fs::canonicalize(root).unwrap());
        fs::copy(
            fixture_root().join("graphql/schema.graphql"),
            value.join("graphql/schema.graphql"),
        )
        .unwrap();
        let manifest = fs::read_to_string(fixture_root().join("chigiri.toml")).unwrap();
        let manifest = if binding {
            &manifest
        } else {
            manifest.split("[bindings]").next().unwrap()
        };
        fs::write(value.join("chigiri.toml"), manifest).unwrap();
        value
    }

    pub(super) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    pub(super) fn success(&self, action: &str) {
        let output = super::cargo::run(
            Command::new(env!("CARGO_BIN_EXE_chigiri"))
                .args(["rust", "--manifest"])
                .arg(self.join("chigiri.toml"))
                .args([action, "--format", "json", "--deny-warnings"])
                .env("CARGO_NET_OFFLINE", "true")
                .env(
                    "CARGO_TARGET_DIR",
                    fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                        .unwrap()
                        .join("target/sibling-generator/fixtures-cache"),
                ),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    pub(super) fn test_consumer(&self) {
        fs::create_dir_all(self.join("consumer/src")).unwrap();
        for entry in fs::read_dir(fixture_root().join("consumer/src")).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(
                entry.path(),
                self.join("consumer/src").join(entry.file_name()),
            )
            .unwrap();
        }
        for name in ["consumer/Cargo.toml", "consumer/Cargo.lock"] {
            fs::copy(fixture_root().join(name), self.join(name)).unwrap();
        }
        let output = super::cargo::run(
            super::cargo::command(&self.join("consumer/Cargo.toml"), "test")
                .args(["--all-targets", "--all-features"]),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("res_01_resolver_expressions_are_evaluated_once_in_source_order"));
        assert!(stdout.contains("1 passed; 0 failed"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn generated_files(root: &Path) -> BTreeMap<String, String> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                files.insert(name, fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}
