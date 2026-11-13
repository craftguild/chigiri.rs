//! Verifies that built-in definitions survive codegen/check round trips without preprocessing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chigiri_codegen::{ProcessLimits, ProcessOutput, ResolvedManifest, run_bounded_process};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = fixture("../../target/sibling-generator").join(format!(
            "chigiri-vnext-builtins-{label}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(path.join("schema")).unwrap();
        let value = Self(fs::canonicalize(path).unwrap());
        fs::write(
            value.join("schema/schema.graphql"),
            "type Query { value: String }\n",
        )
        .unwrap();
        fs::write(value.join("chigiri.toml"), concat!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"schema\"\n",
            "[package]\nname=\"builtin-import-contract\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
            "[generation]\noutput=\"generated\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n"
        )).unwrap();
        value
    }

    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    fn cli(&self, action: &str, format: &str) -> ProcessOutput {
        run_bounded_process(
            Command::new(env!("CARGO_BIN_EXE_chigiri"))
                .args(["rust", "--manifest"])
                .arg(self.join("chigiri.toml"))
                .args([action, "--deny-warnings", "--format", format])
                .env("CARGO_NET_OFFLINE", "true")
                .env(
                    "CARGO_TARGET_DIR",
                    fs::canonicalize(fixture("../.."))
                        .unwrap()
                        .join("target/sibling-generator/fixtures-cache"),
                ),
            &[],
            ProcessLimits {
                timeout: Duration::from_secs(300),
                bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap()
    }

    fn generate(&self) {
        let output = self.cli("codegen", "human");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(path: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if path.starts_with("tests/") {
        root.join("../codegen").join(path)
    } else {
        root.join(path)
    }
}

fn descriptor_files(directory: &Path) -> BTreeMap<String, String> {
    let files = directory_files(directory)
        .into_iter()
        .filter(|(name, _)| name.starts_with("src/__chigiri/internal/descriptor/"))
        .map(|(name, bytes)| {
            (
                name,
                String::from_utf8(bytes).expect("The descriptor is UTF-8."),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert!(files.contains_key("src/__chigiri/internal/descriptor/schema.rs"));
    files
}

fn semantic_descriptor_files(directory: &Path) -> BTreeMap<String, String> {
    descriptor_files(directory)
        .into_iter()
        .map(|(name, mut source)| {
            if let Some(start) = source.find("schema_digest: ::chigiri::SchemaDigest::from_bytes(")
            {
                let end = source[start..]
                    .find("]),\n")
                    .map(|offset| start + offset + 4)
                    .expect("ABI 8 digest is closed.");
                source.replace_range(start..end, "schema_digest: <exact-source>,\n");
            }
            (name, source)
        })
        .collect()
}

fn directory_files(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("directory must be readable") {
            let entry = entry.expect("directory entry must be readable");
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(name, fs::read(entry.path()).expect("file must be readable"));
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(directory, directory, &mut files);
    files
}

#[test]
fn async_graphql_check_generation_does_not_require_manual_snapshot_processing() {
    let temporary = TestDirectory::new("snapshot");
    fs::copy(
        fixture("tests/fixtures/builtin-imports/schema.graphql"),
        temporary.join("schema/schema.graphql"),
    )
    .unwrap();
    let generated = temporary.join("generated");

    temporary.generate();
    let manifest = ResolvedManifest::load(&temporary.join("chigiri.toml")).unwrap();
    chigiri_codegen::check_contract(&manifest)
        .expect("built-in definitions must pass a repeated drift check");

    let sources = fs::read_to_string(generated.join("src/__chigiri/sources.rs"))
        .expect("source provenance must be readable");
    assert!(sources.contains("schema.graphql"));
}

#[test]
fn sdl_01_builtin_reordering_and_descriptor_fingerprint_do_not_change_inputs() {
    let plain = TestDirectory::new("plain-descriptor");
    let imported = TestDirectory::new("imported-descriptor");
    fs::copy(
        fixture("tests/fixtures/builtin-imports/schema.graphql"),
        imported.join("schema/schema.graphql"),
    )
    .unwrap();
    plain.generate();
    imported.generate();
    let plain_output = plain.join("generated");
    let imported_output = imported.join("generated");

    assert_eq!(
        semantic_descriptor_files(&plain_output),
        semantic_descriptor_files(&imported_output)
    );
    assert_ne!(
        descriptor_files(&plain_output),
        descriptor_files(&imported_output)
    );
}

#[test]
fn sdl_04_05_builtin_competition_is_non_zero_and_does_not_change_existing_output() {
    assert_builtin_conflict(
        "skip",
        "directive @skip(if: Boolean) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT\n\
         type Query { value: String }\n",
    );
    assert_builtin_conflict(
        "deprecated",
        "directive @deprecated(reason: String! = \"different\") on FIELD_DEFINITION | \
         ARGUMENT_DEFINITION | INPUT_FIELD_DEFINITION | ENUM_VALUE\n\
         type Query { value: String }\n",
    );
}

fn assert_builtin_conflict(name: &str, source: &str) {
    let temporary = TestDirectory::new(name);
    let generated = temporary.join("generated");
    temporary.generate();
    let before = directory_files(&generated);
    fs::write(temporary.join("schema/schema.graphql"), source).unwrap();
    let output = temporary.cli("update", "human");
    let stderr = String::from_utf8(output.stderr).expect("The diagnostic is UTF-8.");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("error[SF2004]"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "Built-in directive definition '@{name}' conflicts with GraphQL September 2025"
        )),
        "{stderr}"
    );
    assert!(stderr.contains("schema.graphql:1:12"), "{stderr}");
    // Human output omits source text, while JSON preserves the exact source span.
    let json = temporary.cli("update", "json");
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&json.stderr).unwrap();
    assert_eq!(report["schema_version"], "chigiri/diagnostics-v2");
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["code"], "SF2004");
    let label = &diagnostic["labels"][0];
    assert_eq!(label["path"], "schema.graphql");
    assert_eq!(label["line"], 1);
    assert_eq!(label["column"], 12);
    assert_eq!(label["start"], 11);
    assert_eq!(label["end"], 11 + name.len());
    assert_eq!(directory_files(&generated), before);
}
