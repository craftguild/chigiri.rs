//! Verifies bounded current inputs against recorded provenance before installing a candidate.

use crate::filesystem::CapturedTree;
use crate::{CodegenError, ResolvedManifest};

pub(in crate::contract) fn verify(
    manifest: &ResolvedManifest,
    tree: &CapturedTree,
) -> Result<(), CodegenError> {
    manifest.verify_paths()?;
    let marker: super::Marker = serde_json::from_slice(
        tree.files
            .get(crate::GENERATED_MARKER_FILE)
            .ok_or_else(changed)?,
    )
    .map_err(|_| changed())?;
    let sources = crate::discovery::discover_captured(
        &manifest.schema,
        &chigiri_core::ExecutionLimits::default(),
    )?;
    if marker.provenance.manifest_sha256 != super::digest(manifest.manifest_bytes())
        || marker.provenance.sources.len() != sources.len()
        || marker
            .provenance
            .sources
            .iter()
            .zip(&sources)
            .any(|(record, source)| {
                record.path != source.relative_path
                    || record.bytes != source.contents.len() as u64
                    || record.sha256 != super::digest(source.contents.as_bytes())
            })
    {
        return Err(changed());
    }
    manifest.verify_paths()
}

fn changed() -> CodegenError {
    CodegenError::new(
        "SF4020",
        "The manifest or SDL input was changed after the candidate was created.",
    )
}
