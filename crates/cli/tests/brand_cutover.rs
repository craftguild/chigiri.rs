//! Prove the clean brand break and fresh-destination migration through the product binary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chigiri-brand");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "cutover-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn run(&self, current: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_chigiri"))
            .args(args)
            .current_dir(self.0.join(current))
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap()
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn snapshot(&self, path: &str) -> BTreeMap<String, Vec<u8>> {
        snapshot(&self.0.join(path))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

fn manifest(brand: &str) -> String {
    format!(
        "schema_version=\"{brand}/config-v2\"\n\
         [schema]\ndirectory=\"schema\"\n\
         [package]\nname=\"brand-contract\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\
         [generation]\noutput=\"generated\"\n\
         [runtime]\nsource=\"path\"\nroot=\"../../../..\"\n"
    )
}

#[test]
fn ch_09_rust_rejects_old_and_mixed_ownership_before_writes_then_regenerates_fresh() {
    let fixture = Fixture::new();
    fixture.write(
        "legacy/schema/schema.graphql",
        "type Query { value: String! }\n",
    );
    fixture.write("legacy/pythia.toml", manifest("pythia"));
    let legacy = fixture.snapshot("legacy");

    for args in [
        vec!["rust", "codegen"],
        vec!["rust", "--manifest", "pythia.toml", "codegen"],
    ] {
        assert!(!fixture.run("legacy", &args).status.success(), "{args:?}");
        assert_eq!(fixture.snapshot("legacy"), legacy);
    }

    fixture.write(
        "fresh/schema/schema.graphql",
        "type Query { value: String! }\n",
    );
    fixture.write("fresh/chigiri.toml", manifest("chigiri"));
    let generated = fixture.run("fresh", &["rust", "codegen", "--deny-warnings"]);
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(
        fixture
            .run("fresh", &["rust", "check", "--deny-warnings"])
            .status
            .success()
    );
    assert_eq!(fixture.snapshot("legacy"), legacy);

    let marker = "fresh/generated/chigiri-generated.json";
    let current = fs::read_to_string(fixture.0.join(marker)).unwrap();
    fixture.write(
        marker,
        current.replace("chigiri/generated-crate-v1", "pythia/generated-crate-v1"),
    );
    let mixed = fixture.snapshot("fresh");
    assert!(
        !fixture
            .run("fresh", &["rust", "update", "--deny-warnings"])
            .status
            .success()
    );
    assert_eq!(fixture.snapshot("fresh"), mixed);
}

#[test]
fn ch_09_cv_06_mock_rejects_old_and_mixed_inputs_then_initializes_a_separate_project() {
    let fixture = Fixture::new();
    let current = include_str!("../../mock/fixtures/mock-v11-ready.toml");
    for (name, source) in [
        (
            "retired.toml",
            current.replace("chigiri-mock/v11", "chigiri-mock/v9"),
        ),
        (
            "old.toml",
            current.replace("chigiri-mock/v11", "pythia-mock/v8"),
        ),
        (
            "mixed.toml",
            current.replace(
                "chigiri-mock-capabilities/v4",
                "pythia-mock-capabilities/v2",
            ),
        ),
    ] {
        assert_ne!(source, current, "the rejected bundle must change the input");
        fixture.write(name, &source);
        let before = fixture.snapshot(".");
        assert!(
            !fixture
                .run(".", &["mock", "check", "--config", name])
                .status
                .success()
        );
        assert_eq!(fixture.snapshot("."), before);
    }

    let old_bundle = fs::read(fixture.0.join("old.toml")).unwrap();
    fixture.write(
        "schema.graphql",
        include_str!("../../mock/fixtures/blog.graphql"),
    );
    let initialized = fixture.run(
        ".",
        &[
            "mock",
            "init",
            "--schema",
            "schema.graphql",
            "--config-dir",
            "fresh-mock",
        ],
    );
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    assert_eq!(fs::read(fixture.0.join("old.toml")).unwrap(), old_bundle);
    let config = "fresh-mock/mock.luau";
    let source = fs::read_to_string(fixture.0.join(config)).unwrap();
    assert!(source.contains("chigiri-mock-project/v10"));
    let rejected = source.replace("chigiri-mock-project/v10", "pythia-mock-project/v6");
    assert_ne!(
        rejected, source,
        "the rejected project must change the input"
    );
    fixture.write(config, rejected);
    let mixed_project = fixture.snapshot("fresh-mock");
    assert!(
        !fixture
            .run(".", &["mock", "update", "--config-dir", "fresh-mock"])
            .status
            .success()
    );
    assert_eq!(fixture.snapshot("fresh-mock"), mixed_project);
}
