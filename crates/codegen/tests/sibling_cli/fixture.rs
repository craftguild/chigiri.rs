//! Runs the CLI with isolated inputs and compares every output byte.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chigiri_codegen::{ProcessLimits, ProcessOutput, run_bounded_process};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    repository: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let repository =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        let root = repository.join("target/sibling-generator").join(format!(
            "cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("graphql")).unwrap();
        fs::write(
            root.join("graphql/schema.graphql"),
            "type Query { value: String }",
        )
        .unwrap();
        fs::write(root.join("selected.toml"), concat!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n",
            "[package]\nname=\"sibling-cli-contract\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
            "[generation]\noutput=\"generated/contract\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n"
        )).unwrap();
        Self { root, repository }
    }

    pub(super) fn output(&self) -> PathBuf {
        self.root.join("generated/contract")
    }

    pub(super) fn command(&self, command: &str) -> Command {
        self.command_with_policy(command, true)
    }

    pub(super) fn command_with_policy(&self, command: &str, deny_warnings: bool) -> Command {
        let mut process = Command::new(env!("CARGO_BIN_EXE_chigiri"));
        process
            .current_dir(&self.root)
            .args([
                "rust",
                "--manifest",
                "selected.toml",
                command,
                "--format",
                "json",
            ])
            .env("CARGO_NET_OFFLINE", "true")
            .env(
                "CARGO_TARGET_DIR",
                self.repository.join("target/sibling-generator/cli-cache"),
            )
            .env("CHIGIRI_SCHEMA_DIR", "wrong-schema")
            .env("CHIGIRI_OUTPUT_DIR", "wrong-output")
            .env("CHIGIRI_RUNTIME_ROOT", "wrong-runtime");
        if deny_warnings {
            process.arg("--deny-warnings");
        }
        process
    }

    pub(super) fn execute(&self, process: &mut Command) -> ProcessOutput {
        run_bounded_process(
            process,
            &[],
            ProcessLimits {
                timeout: Duration::from_secs(60),
                bytes: 1024 * 1024,
            },
        )
        .unwrap()
    }

    pub(super) fn run(&self, command: &str) -> ProcessOutput {
        self.execute(&mut self.command(command))
    }

    pub(super) fn success(&self, command: &str) {
        let output = self.run(command);
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    pub(super) fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut directories = vec![self.output()];
        let mut files = BTreeMap::new();
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    directories.push(path);
                } else {
                    files.insert(
                        path.strip_prefix(self.output()).unwrap().to_owned(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        files
    }

    pub(super) fn wrapper(&self, tool: &str, script: &str) -> std::ffi::OsString {
        let bin = self.root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let path = bin.join(tool);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        std::env::join_paths(paths).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn report(output: &ProcessOutput) -> serde_json::Value {
    assert!(output.stdout.is_empty());
    assert!(output.stderr.len() <= chigiri_codegen::MAX_DIAGNOSTIC_REPORT_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["schema_version"], "chigiri/diagnostics-v2");
    assert_eq!(value.as_object().unwrap().len(), 3);
    value
}
