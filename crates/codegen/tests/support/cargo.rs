//! Runs fixture Cargo commands with reusable caches and bounded child time and output.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use chigiri_codegen::{ProcessLimits, ProcessOutput, run_bounded_process};

static CARGO_COMMAND: Mutex<()> = Mutex::new(());

pub(super) fn command(manifest: &Path, action: &str) -> Command {
    let repository = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let mut command = Command::new(env!("CARGO"));
    command
        .args([action, "--offline", "--locked", "--manifest-path"])
        .arg(manifest)
        .env(
            "CARGO_TARGET_DIR",
            repository.join("target/sibling-generator/fixtures-cache"),
        );
    command
}

pub(super) fn run(command: &mut Command) -> ProcessOutput {
    // Fixture commands share a target directory, so concurrent commands would
    // spend their bounded runtime waiting for Cargo's package-cache lock.
    let _guard = CARGO_COMMAND
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    run_bounded_process(
        command,
        &[],
        ProcessLimits {
            timeout: Duration::from_secs(300),
            bytes: 64 * 1024 * 1024,
        },
    )
    .unwrap()
}
