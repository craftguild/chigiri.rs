//! Build manifests, isolated inputs, independent SDKs, and handwritten consumers.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chigiri_codegen::{CodegenError, ResolvedManifest};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TestDirectory(PathBuf);

impl TestDirectory {
    pub(super) fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = fixture("../../target/sibling-generator").join(format!(
            "chigiri-generation-{label}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        let value = Self(fs::canonicalize(path).unwrap());
        value.write_manifest();
        value
    }

    pub(super) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    fn write_manifest(&self) {
        fs::write(self.join("chigiri.toml"), concat!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"schema\"\n",
            "[package]\nname=\"generation-contract\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
            "[generation]\noutput=\"generated\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n"
        )).unwrap();
    }

    pub(super) fn manifest(&self) -> ResolvedManifest {
        ResolvedManifest::load(&self.join("chigiri.toml")).unwrap()
    }

    pub(super) fn copy_schema(&self, directory: &Path) {
        for (name, bytes) in read_files(directory) {
            let output = self.join("schema").join(name);
            fs::create_dir_all(output.parent().unwrap()).unwrap();
            fs::write(output, bytes).unwrap();
        }
    }

    pub(super) fn cli(&self, action: &str) -> chigiri_codegen::ProcessOutput {
        super::cargo::run(
            Command::new(env!("CARGO_BIN_EXE_chigiri"))
                .args(["rust", "--manifest"])
                .arg(self.join("chigiri.toml"))
                .args([action, "--format", "json", "--deny-warnings"])
                .env("CARGO_NET_OFFLINE", "true")
                .env(
                    "CARGO_TARGET_DIR",
                    fs::canonicalize(fixture("../.."))
                        .unwrap()
                        .join("target/sibling-generator/fixtures-cache"),
                ),
        )
    }

    pub(super) fn generate(&self) {
        let output = self.cli("codegen");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        chigiri_codegen::check_contract(&self.manifest()).unwrap();
    }

    pub(super) fn render_error(&self) -> CodegenError {
        match chigiri_codegen::render_contract(&self.manifest()) {
            Ok(_) => panic!("invalid schema unexpectedly rendered"),
            Err(error) => error,
        }
    }

    pub(super) fn write_consumer(&self, name: &str, source: &str) {
        self.write_consumer_with_dependencies(name, source, "");
    }

    pub(super) fn write_consumer_with_dependencies(
        &self,
        name: &str,
        source: &str,
        dependencies: &str,
    ) {
        fs::create_dir_all(self.join("consumer/src")).unwrap();
        fs::write(self.join("consumer/src/lib.rs"), source).unwrap();
        fs::write(
            self.join("consumer/Cargo.toml"),
            format!(
                "[package]\nname=\"{name}\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n\
             [dependencies]\ngenerated={{package=\"generation-contract\",path=\"../generated\"}}\n\
             chigiri={{path=\"../../../../crates/facade\"}}\n{dependencies}"
            ),
        )
        .unwrap();
    }

    pub(super) fn assert_consumer(&self, action: &str) {
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
        let output = super::cargo::run(&mut super::cargo::command(&manifest, action));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn fixture(path: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if path.starts_with("tests/") {
        root.join("../codegen").join(path)
    } else {
        root.join(path)
    }
}

pub(super) fn read_files(directory: &Path) -> BTreeMap<String, Vec<u8>> {
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
    collect(directory, directory, &mut files);
    files
}
