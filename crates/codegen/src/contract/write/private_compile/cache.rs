//! Resolves Cargo target settings and redirects only the default temporary target to a reusable
//! cache.

use std::path::Path;

use crate::filesystem::{Directory, ResolvedPath};
use crate::{CodegenError, ResolvedManifest};

use super::super::transaction::Transaction;

pub(super) fn resolve(
    manifest: &ResolvedManifest,
    transaction: &Transaction,
    private: &Directory,
    bytes: &[u8],
) -> Result<ResolvedPath, CodegenError> {
    let metadata: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let target = metadata
        .get("target_directory")
        .and_then(|value| value.as_str())
        .ok_or_else(invalid)?;
    if !Path::new(target).is_absolute() {
        return Err(invalid());
    }
    let target = manifest
        .root
        .resolve_destination(Path::new(target))
        .map_err(|_| invalid())?;
    let default_target = private
        .descendant_destination(
            &crate::filesystem::PortableRelativePath::new("target").map_err(invalid_io)?,
        )
        .map_err(invalid_io)?;
    let target = if target == default_target {
        // Move only Cargo's default target outside the staged SDK. Explicit target
        // settings remain application-owned and are validated in place.
        transaction
            .parent
            .descendant_destination(
                &crate::filesystem::PortableRelativePath::new(".chigiri-cache/target")
                    .map_err(invalid_io)?,
            )
            .map_err(invalid_io)?
    } else {
        target
    };
    validate(manifest, transaction, &target)?;
    if let Some(build) = metadata.get("build_directory") {
        let build = build.as_str().ok_or_else(invalid)?;
        if !Path::new(build).is_absolute() {
            return Err(invalid());
        }
        let build = manifest
            .root
            .resolve_destination(Path::new(build))
            .map_err(|_| invalid())?;
        // Cargo's default build directory follows the target-directory setting.
        if build != default_target {
            validate(manifest, transaction, &build)?;
        }
    }
    Ok(target)
}

fn validate(
    manifest: &ResolvedManifest,
    transaction: &Transaction,
    path: &ResolvedPath,
) -> Result<(), CodegenError> {
    let output =
        crate::filesystem::PortableRelativePath::new(&transaction.name).map_err(invalid_io)?;
    for forbidden in [
        transaction
            .parent
            .descendant_destination(&output)
            .map_err(invalid_io)?,
        transaction.stage.destination().map_err(invalid_io)?,
        manifest.schema.destination().map_err(invalid_io)?,
    ] {
        if manifest
            .root
            .overlaps_destination(path, &forbidden)
            .map_err(|_| invalid())?
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn invalid_io(_: crate::filesystem::FsError) -> CodegenError {
    invalid()
}

fn invalid() -> CodegenError {
    CodegenError::new(
        "SF4023",
        "Cargo target and build directories must not overlap the generated SDK, schema inputs, or private staging directory.",
    )
}
