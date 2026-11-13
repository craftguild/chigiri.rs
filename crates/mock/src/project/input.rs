//! Apply the same no-follow reads and byte snapshots to portable bundles and explicit SDL inputs.

use std::path::Path;

use super::filesystem::{Directory, FileSnapshot};
use super::{ProjectError, metadata};

/// Input file retaining bytes and identity captured by the initial read.
pub struct InputFile(FileSnapshot);

impl InputFile {
    /// Reject a final symlink or FIFO and read through the same validated descriptor.
    pub fn read(path: &Path, maximum: usize) -> Result<Self, ProjectError> {
        if maximum > metadata::MAX_BYTES {
            return Err(ProjectError::new("limit", "input file byte limit"));
        }
        let directory = Directory::open(if path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        })?;
        directory.read(path, maximum).map(Self)
    }

    /// Return UTF-8 bytes captured by the initial read.
    pub fn source(&self) -> &str {
        &self.0.bytes
    }

    /// Return the identity used to reject duplicate access to the same file.
    pub fn identity(&self) -> chigiri_host::filesystem::FileIdentity {
        self.0.identity()
    }

    /// Check for concurrent changes against the initial snapshot after parsing and type checking.
    pub fn verify(&self) -> Result<(), ProjectError> {
        self.0.verify()
    }
}
