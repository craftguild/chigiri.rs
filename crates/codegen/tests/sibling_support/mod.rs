//! Shared Cargo environment and private-compile layout for sibling contract tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chigiri_codegen::{ProcessLimits, RenderedContract, run_bounded_process};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture(pub PathBuf);

impl Fixture {
    pub fn new() -> Self {
        let path = Self::repository()
            .join("target/sibling-generator")
            .join(format!(
                "contract-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn repository() -> PathBuf {
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
    }

    pub fn prepare(&self, schema: &str, enums: &[&str]) {
        fs::create_dir(self.0.join("graphql")).unwrap();
        fs::write(self.0.join("graphql/schema.graphql"), schema).unwrap();
        let names = enums
            .iter()
            .map(|name| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(self.0.join("chigiri.toml"), format!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n[package]\nname=\"sibling-contract\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[generation]\noutput=\"generated/contract\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n[bindings]\ndomain_enums=[{names}]\n"
        )).unwrap();
    }

    pub fn write_rendered(&self, rendered: &RenderedContract) {
        assert!(rendered.files().len() <= 65_536);
        assert!(
            rendered
                .files()
                .map(|(_, bytes)| bytes.len())
                .sum::<usize>()
                <= 64 * 1024 * 1024
        );
        // Mirror the same relative depth so every byte, including Cargo paths, remains unchanged.
        for root in ["generated/contract", "private/contract"] {
            let root = self.0.join(root);
            if root.exists() {
                fs::remove_dir_all(&root).unwrap();
            }
            for (name, bytes) in rendered.files() {
                let target = root.join(name);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(target, bytes).unwrap();
            }
        }
    }

    pub fn prepare_consumer(&self, source: &str) {
        fs::create_dir_all(self.0.join("domain/src")).unwrap();
        fs::write(self.0.join("domain/Cargo.toml"), "[package]\nname=\"sibling-domain\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n").unwrap();
        fs::write(
            self.0.join("domain/src/lib.rs"),
            include_str!("../fixtures/sibling-enum/domain.rs"),
        )
        .unwrap();
        fs::create_dir_all(self.0.join("consumer/src")).unwrap();
        fs::write(self.0.join("consumer/src/lib.rs"), source).unwrap();
        let runtime =
            toml_edit::Value::from(Self::repository().join("crates/facade").to_str().unwrap())
                .to_string();
        fs::write(self.0.join("consumer/Cargo.toml"), format!(
            "[package]\nname=\"sibling-consumer\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\ncontract={{package=\"sibling-contract\",path=\"../generated/contract\"}}\nexternal_domain={{package=\"sibling-domain\",path=\"../domain\"}}\nchigiri={{path={runtime}}}\n"
        )).unwrap();
    }

    pub fn cargo(&self, manifest: &str, args: &[&str], success: bool) -> String {
        let split = args
            .iter()
            .position(|arg| *arg == "--")
            .unwrap_or(args.len());
        let mut command = Command::new(env!("CARGO"));
        command
            .args(&args[..split])
            .args(["--offline", "--manifest-path"])
            .arg(self.0.join(manifest))
            .env(
                "CARGO_TARGET_DIR",
                Self::repository().join("target/sibling-generator/contract-cache"),
            )
            .env("RUSTDOCFLAGS", "-D warnings");
        if split < args.len() {
            command.arg("--").args(&args[split + 1..]);
        }
        let output = run_bounded_process(
            &mut command,
            &[],
            ProcessLimits {
                timeout: Duration::from_secs(300),
                bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.success(), success, "{manifest}: {text}");
        text
    }

    pub fn file<'a>(&self, rendered: &'a RenderedContract, path: &str) -> &'a [u8] {
        rendered.files().find(|(name, _)| *name == path).unwrap().1
    }

    pub fn format_check(&self, rendered: &RenderedContract) {
        let paths = rendered
            .files()
            .filter(|(path, _)| path.ends_with(".rs"))
            .map(|(path, _)| self.0.join("private/contract").join(path))
            .collect::<Vec<_>>();
        for batch in paths.chunks(128) {
            let output = run_bounded_process(
                Command::new("rustup")
                    .args(["run", "1.95.0", "rustfmt", "--check", "--config-path"])
                    .arg(Self::repository().join("crates/codegen/rustfmt.generated.toml"))
                    .args(batch),
                &[],
                ProcessLimits {
                    timeout: Duration::from_secs(300),
                    bytes: 64 * 1024 * 1024,
                },
            )
            .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
