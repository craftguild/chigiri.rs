//! Manages trait-surface test inputs and outputs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TestDirectory(PathBuf);

impl TestDirectory {
    pub(super) fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = manifest_root()
            .join("../../target/sibling-generator")
            .join(format!(
                "chigiri-usability-trait-surface-{label}-{}-{serial}",
                std::process::id()
            ));
        fs::create_dir_all(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    pub(super) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    pub(super) fn manifest(&self) -> chigiri_codegen::ResolvedManifest {
        chigiri_codegen::ResolvedManifest::load(&self.join("chigiri.toml")).unwrap()
    }

    pub(super) fn generate(&self, schema: &Path) {
        // Copy the original SDL path and bytes into the default input.
        for (name, bytes) in generated_files(schema) {
            let destination = self.join("schema").join(name);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, bytes).unwrap();
        }
        fs::write(self.join("chigiri.toml"), concat!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"schema\"\n",
            "[package]\nname=\"trait-surface-contract\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
            "[generation]\noutput=\"generated\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n"
        )).unwrap();
        let output = super::cargo::run(
            Command::new(env!("CARGO_BIN_EXE_chigiri"))
                .args(["rust", "--manifest"])
                .arg(self.join("chigiri.toml"))
                .args(["codegen", "--deny-warnings", "--format", "json"])
                .env("CARGO_NET_OFFLINE", "true")
                .env(
                    "CARGO_TARGET_DIR",
                    fs::canonicalize(manifest_root().join("../.."))
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
        chigiri_codegen::check_contract(&self.manifest()).unwrap();
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn manifest_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen")
}

pub(super) fn generated_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(name, fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}
