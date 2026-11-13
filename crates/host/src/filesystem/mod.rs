//! Capability-style filesystem operations with closed failure classes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{HostError, PortableRelativePath};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::{
    BoundedWriter, Directory, DirectoryCreationFailure, DirectoryLock, FileObservation,
    FileSnapshot, ObservedFile, OwnedFile, RenameFailure, SnapshotMoveFailure, TemporaryFile,
};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::{
    BoundedWriter, Directory, DirectoryCreationFailure, DirectoryLock, FileObservation,
    FileSnapshot, ObservedFile, OwnedFile, RenameFailure, SnapshotMoveFailure, TemporaryFile,
};

/// Closed filesystem failure classification.
pub type FsError = HostError;

/// Opaque stable identity for one opened filesystem entry.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileIdentity(u64, u64);

impl FileIdentity {
    pub(super) fn new(device: u64, inode: u64) -> Self {
        Self(device, inode)
    }
}

/// Opaque native destination validated by a retained directory capability.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedPath(PathBuf);

impl std::fmt::Debug for ResolvedPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResolvedPath { .. }")
    }
}

impl ResolvedPath {
    pub(super) fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }

    /// Appends this validated destination to an argument vector without exposing it.
    pub fn push_argument<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        command.arg(&self.0)
    }
}

/// Fixed resource ceilings for bounded tree capture.
#[derive(Clone, Copy)]
pub struct TreeLimits {
    /// Maximum directory depth.
    pub depth: usize,
    /// Maximum entries examined, including unselected entries.
    pub entries: usize,
    /// Maximum selected regular files.
    pub files: usize,
    /// Maximum bytes in one selected file.
    pub file_bytes: usize,
    /// Maximum bytes across selected files.
    pub total_bytes: usize,
}

impl TreeLimits {
    /// Returns the standard generated-tree ceilings.
    pub fn generated() -> Self {
        Self {
            depth: 64,
            entries: 65_536,
            files: 65_536,
            file_bytes: 64 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Complete bounded logical tree captured through a directory capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedTree {
    /// Canonical logical file paths and retained bytes.
    pub files: BTreeMap<PortableRelativePath, Vec<u8>>,
    /// Canonical logical directory paths.
    pub directories: BTreeSet<PortableRelativePath>,
}

impl CapturedTree {
    /// Validates string keys and builds their portable logical inventory.
    pub fn try_from_files(files: BTreeMap<String, Vec<u8>>) -> Result<Self, FsError> {
        let files = files
            .into_iter()
            .map(|(path, bytes)| PortableRelativePath::try_from(path).map(|path| (path, bytes)))
            .collect::<Result<_, _>>()?;
        let tree = Self::from_files(files);
        tree.validate_write()?;
        Ok(tree)
    }

    /// Builds the directory inventory implied by canonical file paths.
    pub fn from_files(files: BTreeMap<PortableRelativePath, Vec<u8>>) -> Self {
        let mut directories = BTreeSet::new();
        for path in files.keys() {
            let mut parent = path.as_str();
            while let Some((ancestor, _)) = parent.rsplit_once('/') {
                directories.insert(
                    PortableRelativePath::new(ancestor)
                        .expect("an ancestor of a portable path is portable"),
                );
                parent = ancestor;
            }
        }
        Self { files, directories }
    }

    pub(super) fn validate_write(&self) -> Result<(), FsError> {
        if self.files.len() + self.directories.len() > 65_536 {
            return Err(FsError::Limit);
        }
        let mut total = 0usize;
        for bytes in self.files.values() {
            total = total.checked_add(bytes.len()).ok_or(FsError::Limit)?;
            if total > 64 * 1024 * 1024 {
                return Err(FsError::Limit);
            }
        }
        for directory in &self.directories {
            if self.files.contains_key(directory.as_str()) {
                return Err(FsError::Path);
            }
        }
        for file in self.files.keys() {
            if let Some((parent, _)) = file.rsplit_once('/')
                && !self.directories.contains(parent)
            {
                return Err(FsError::Path);
            }
        }
        for directory in &self.directories {
            if let Some((parent, _)) = directory.rsplit_once('/')
                && !self.directories.contains(parent)
            {
                return Err(FsError::Path);
            }
        }
        Ok(())
    }
}

/// Validates and splits one portable logical path.
pub fn relative_components(value: &str) -> Result<Vec<&str>, FsError> {
    PortableRelativePath::new(value)?;
    Ok(value.split('/').collect())
}
