//! Compile candidate Rust bytes in a private crate using validated Cargo settings and caches.

mod cache;

use std::process::Command;
use std::time::Duration;

use crate::filesystem::{CapturedTree, Directory, FsError, TreeLimits};
use crate::{
    CodegenError, DiagnosticReport, ProcessLimits, ProcessOutput, ResolvedManifest,
    run_bounded_process,
};

use super::transaction::{Transaction, combine, io};

pub(super) fn preflight(manifest: &ResolvedManifest) -> Result<(), CodegenError> {
    manifest.verify_paths()?;
    for command in [
        Command::new("cargo").arg("--version"),
        Command::new("rustup").args(["run", "1.95.0", "rustfmt", "--version"]),
    ] {
        manifest.root.set_current_dir(command).map_err(io)?;
        let output = run_bounded_process(
            command,
            &[],
            ProcessLimits {
                timeout: Duration::from_secs(300),
                bytes: 64 * 1024,
            },
        )
        .map_err(process_error)?;
        if !output.status.success() {
            return Err(tool());
        }
    }
    Ok(())
}

pub(super) fn check(
    manifest: &ResolvedManifest,
    transaction: &Transaction,
) -> Result<DiagnosticReport, CodegenError> {
    let private = transaction.stage.create_directory("compile").map_err(io)?;
    let mut expected = transaction
        .expected
        .as_ref()
        .ok_or_else(|| io(FsError::Missing))?
        .clone();
    expected.files.insert(
        crate::filesystem::PortableRelativePath::new("Cargo.toml").map_err(io)?,
        crate::contract::cargo::render_at(manifest, &private)?.into_bytes(),
    );
    let result = (|| {
        private
            .write_tree_with(&expected, crate::cancellation::filesystem)
            .map_err(io)?;
        let metadata = execute(
            manifest,
            &private,
            &["metadata", "--no-deps", "--format-version=1"],
            None,
        )?;
        if !metadata.status.success() {
            return Err(CodegenError::new(
                "SF4023",
                "Cargo metadata failed for the private generated crate.",
            ));
        }
        let cache = cache::resolve(manifest, transaction, &private, &metadata.stdout)?;
        verify(&private, &expected)?;
        let checked = execute(
            manifest,
            &private,
            &["check", "--message-format=json"],
            Some(&cache),
        )?;
        let report = diagnostics(manifest, &private, &checked)?;
        transaction.verify_candidate()?;
        verify(&private, &expected)?;
        Ok(report)
    })();
    let cleanup = crate::cancellation::recovering(|| cleanup(&private, &expected));
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => Err(combine(
            primary,
            cleanup,
            "Private compile cleanup also failed.",
        )),
    }
}

fn diagnostics(
    manifest: &ResolvedManifest,
    private: &Directory,
    output: &ProcessOutput,
) -> Result<DiagnosticReport, CodegenError> {
    let path = |value: &str| {
        let path = std::path::Path::new(value);
        let relative = if path.is_absolute() {
            private.emitted_label(path).ok()
        } else {
            crate::filesystem::PortableRelativePath::new(value).ok()
        };
        relative.map_or_else(
            || "<path>".to_owned(),
            |path| format!("{}/{path}", manifest.manifest().output()),
        )
    };
    crate::cargo_diagnostics::compile_output(output, &path).map_err(|error| {
        let compiler_errors = error.report().diagnostics().iter().any(|diagnostic| {
            diagnostic.origin() == crate::DiagnosticOrigin::Rustc
                && diagnostic.severity() == crate::DiagnosticSeverity::Error
        });
        if !compiler_errors {
            return error;
        }
        let mut diagnostics = vec![crate::CodegenDiagnostic::error(
            "SF4025",
            "The generated crate failed to compile.",
        )];
        diagnostics.extend(error.report().diagnostics().iter().cloned());
        CodegenError::from_report(DiagnosticReport::new(
            diagnostics,
            error.report().truncated(),
        ))
    })
}

fn execute(
    manifest: &ResolvedManifest,
    private: &Directory,
    args: &[&str],
    cache: Option<&crate::filesystem::ResolvedPath>,
) -> Result<ProcessOutput, CodegenError> {
    manifest.verify_paths()?;
    private.verify().map_err(io)?;
    let mut command = Command::new("cargo");
    manifest.root.set_current_dir(&mut command).map_err(io)?;
    command.args(args).arg("--manifest-path");
    private
        .push_descendant_argument(
            &mut command,
            &crate::filesystem::PortableRelativePath::new("Cargo.toml").map_err(io)?,
        )
        .map_err(io)?;
    if let Some(cache) = cache {
        command.arg("--target-dir");
        cache.push_argument(&mut command);
    }
    run_bounded_process(
        &mut command,
        &[],
        ProcessLimits {
            timeout: Duration::from_secs(300),
            bytes: 64 * 1024 * 1024,
        },
    )
    .map_err(process_error)
}

fn cleanup(private: &Directory, expected: &CapturedTree) -> Result<(), CodegenError> {
    let actual = private
        .capture_limited(TreeLimits::generated(), |path| {
            Some(if path == "Cargo.lock" {
                1024 * 1024
            } else {
                64 * 1024 * 1024
            })
        })
        .map_err(io)?;
    if !actual.directories.is_subset(&expected.directories)
        || actual.files.iter().any(|(path, bytes)| {
            path.as_str() != "Cargo.lock" && expected.files.get(path) != Some(bytes)
        })
    {
        return Err(io(FsError::Changed));
    }
    private.remove_tree(&actual, &mut || Ok(())).map_err(io)
}

fn verify(private: &Directory, expected: &CapturedTree) -> Result<(), CodegenError> {
    let mut actual = private
        .capture_limited(TreeLimits::generated(), |path| {
            Some(if path == "Cargo.lock" {
                1024 * 1024
            } else {
                64 * 1024 * 1024
            })
        })
        .map_err(io)?;
    actual.files.remove("Cargo.lock");
    if &actual != expected {
        return Err(io(FsError::Changed));
    }
    Ok(())
}

fn tool() -> CodegenError {
    CodegenError::new(
        "SF4008",
        "Unable to run Cargo or rustfmt 1.95.0. Verify that Cargo is installed and install rustfmt with `rustup component add rustfmt --toolchain 1.95.0`.",
    )
}

fn process_error(error: crate::ProcessFailure) -> CodegenError {
    if error == crate::ProcessFailure::Cancelled {
        crate::cancellation::error()
    } else {
        tool()
    }
}
