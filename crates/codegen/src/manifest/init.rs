//! Creates a new exclusive manifest without guessing schema or runtime configuration.

use std::path::Path;

use super::load::{MAX_MANIFEST_BYTES, failure, manifest_parent};
use crate::CodegenError;

/// The same editing manifest as `init`, not based on the cwd.
pub const MANIFEST_TEMPLATE: &str = include_str!("init.toml");

/// Create a new manifest without creating its parent directory.
pub fn init_manifest(path: &Path) -> Result<(), CodegenError> {
    crate::cancellation::check()?;
    let (directory, name) = manifest_parent(path)?;
    directory
        .create(&name, MANIFEST_TEMPLATE.as_bytes(), MAX_MANIFEST_BYTES)
        .map_err(failure)
}
