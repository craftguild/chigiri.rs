//! Reads SDL bytes from an already captured schema-directory handle.

use chigiri_core::ExecutionLimits;

use super::{
    DiscoveredSource, MAX_DIRECTORY_DEPTH, MAX_DIRECTORY_ENTRIES, MAX_SOURCE_FILES,
    MAX_TOTAL_SOURCE_BYTES,
};
use crate::CodegenError;
use crate::filesystem::{Directory, TreeLimits};

pub(crate) fn discover_captured(
    root: &Directory,
    limits: &ExecutionLimits,
) -> Result<Vec<DiscoveredSource>, CodegenError> {
    let captured = root
        .capture(
            TreeLimits {
                depth: MAX_DIRECTORY_DEPTH,
                entries: MAX_DIRECTORY_ENTRIES,
                files: MAX_SOURCE_FILES,
                file_bytes: limits.max_document_bytes.min(256 * 1024),
                total_bytes: MAX_TOTAL_SOURCE_BYTES,
            },
            |path| path.ends_with(".graphql") || path.ends_with(".gql"),
        )
        .map_err(|error| CodegenError::new("SF4002", format!("SDL I/O: {error}")))?;
    if captured.files.is_empty() {
        return Err(CodegenError::new(
            "SF4002",
            "The schema directory contains no .graphql or .gql files.",
        ));
    }
    captured
        .files
        .into_iter()
        .map(|(relative_path, bytes)| {
            let contents = String::from_utf8(bytes)
                .map_err(|_| CodegenError::new("SF4002", "The SDL source is not UTF-8."))?;
            Ok(DiscoveredSource {
                relative_path,
                contents,
            })
        })
        .collect()
}
