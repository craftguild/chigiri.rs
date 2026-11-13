//! Connects the enum-binding manifest, dedicated SDK, and external consumer through real Cargo.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chigiri_codegen::ProcessOutput;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture(PathBuf);

pub(super) fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen/tests/fixtures/enum-binding")
}

pub(super) fn config() -> String {
    // Reuse the manifest runtime while preserving this fixture's input layout.
    include_str!("../fixtures/enum-binding/chigiri.toml")
        .replace("root = \"../../../../..\"", "root = \"../../..\"")
}

impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/sibling-generator")
            .join(format!(
                "enum-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.join("schema")).unwrap();
        let value = Self(fs::canonicalize(root).unwrap());
        fs::copy(
            fixture_root().join("schema/schema.graphql"),
            value.join("schema/schema.graphql"),
        )
        .unwrap();
        fs::write(value.join("chigiri.toml"), config()).unwrap();
        value
    }

    pub(super) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    pub(super) fn cli(&self, action: &str) -> ProcessOutput {
        super::cargo::run(
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
        )
    }

    pub(super) fn success(&self, action: &str) {
        let output = self.cli(action);
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
        for (source, output) in [
            ("consumer.rs", "lib.rs"),
            ("consumer_tests.rs", "consumer_tests.rs"),
        ] {
            fs::copy(
                fixture_root().join(source),
                self.join("consumer/src").join(output),
            )
            .unwrap();
        }
        fs::write(self.join("consumer/Cargo.toml"), concat!(
            "[package]\nname=\"enum-binding-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n",
            "[dependencies]\ngenerated={package=\"enum-binding-contract\",path=\"../generated\"}\n",
            "chigiri={path=\"../../../../crates/facade\"}\n"
        )).unwrap();
        let manifest = self.join("consumer/Cargo.toml");
        let lock = super::cargo::run(
            Command::new(env!("CARGO"))
                .args(["generate-lockfile", "--offline", "--manifest-path"])
                .arg(&manifest),
        );
        assert!(
            lock.status.success(),
            "{}",
            String::from_utf8_lossy(&lock.stderr)
        );
        let output = super::cargo::run(&mut super::cargo::command(&manifest, "test"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
