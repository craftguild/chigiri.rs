//! Fail-closed backend for hosts without reviewed filesystem capabilities.

mod authority;

use std::ffi::OsStr;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use super::{CapturedTree, FileIdentity, FsError, TreeLimits};

#[derive(Clone, Debug)]
struct Unsupported;

#[derive(Clone, Debug)]
/// Unconstructible directory capability on an unreviewed host.
pub struct Directory(Unsupported);

#[derive(Debug)]
/// Fail-closed directory move result.
pub struct RenameFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Always absent on an unsupported host.
    pub moved: Option<Directory>,
}

/// Fail-closed snapshot move result.
pub struct SnapshotMoveFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Always absent on an unsupported host.
    pub moved: Option<Box<FileSnapshot>>,
}

/// Fail-closed directory creation result.
pub struct DirectoryCreationFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Always false on an unsupported host.
    pub created: bool,
}

#[derive(Clone)]
/// Unconstructible file snapshot on an unreviewed host.
pub struct FileSnapshot(Unsupported);

/// Unconstructible file observation on an unreviewed host.
pub struct ObservedFile(Unsupported);

#[derive(Clone)]
/// Unconstructible present-or-absent observation on an unreviewed host.
pub enum FileObservation {
    /// Unreachable present observation.
    Present(Arc<ObservedFile>),
    /// Unreachable absent observation.
    Absent(Directory, std::ffi::OsString),
}

/// Unconstructible directory lock on an unreviewed host.
pub struct DirectoryLock(Unsupported);
/// Unconstructible owned file on an unreviewed host.
pub struct OwnedFile(Unsupported);
/// Unconstructible temporary file on an unreviewed host.
pub struct TemporaryFile(Unsupported);
/// Unconstructible bounded writer on an unreviewed host.
pub struct BoundedWriter<'a>(PhantomData<&'a mut ()>);

impl BoundedWriter<'_> {
    /// Fails closed because no reviewed writer backend exists.
    pub fn write(&mut self, _: &[u8]) -> Result<(), FsError> {
        unsupported()
    }
}

impl Directory {
    /// Fails closed because no reviewed directory backend exists.
    pub fn open(_: &Path) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed directory backend exists.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Returns no identity; instances cannot be constructed safely.
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(0, 0)
    }

    /// Fails closed because no reviewed directory backend exists.
    pub fn child(&self, _: &OsStr) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed file backend exists.
    pub fn read(&self, _: &str, _: usize) -> Result<Vec<u8>, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn create(&self, _: &str, _: &[u8], _: usize) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed traversal backend exists.
    pub fn relative(&self, _: &str) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed traversal backend exists.
    pub fn parent_of(&self, _: &str) -> Result<(Self, String), FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed synchronization backend exists.
    pub fn sync(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because absence cannot be verified.
    pub fn absent(&self, _: &str) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because absence cannot be verified.
    pub fn absent_os(&self, _: &OsStr) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn create_directory(&self, _: &str) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn create_directory_os(&self, _: &OsStr) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed and reports that no creation happened.
    pub fn create_directory_observed(&self, _: &OsStr) -> Result<Self, DirectoryCreationFailure> {
        Err(DirectoryCreationFailure {
            error: FsError::Unsupported,
            created: false,
        })
    }

    /// Fails closed because no reviewed traversal backend exists.
    pub fn optional_child(&self, _: &OsStr) -> Result<Option<Self>, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn ensure_parent(&self, _: &str) -> Result<(Self, String), FsError> {
        unsupported()
    }

    /// Fails closed and reports that no move happened.
    pub fn rename_to(&self, _: &Self, _: &str) -> Result<Self, RenameFailure> {
        Err(RenameFailure {
            error: FsError::Unsupported,
            moved: None,
        })
    }

    #[doc(hidden)]
    pub fn rename_with_hook(
        &self,
        _: &Self,
        _: &str,
        _: impl FnOnce() -> Result<(), FsError>,
    ) -> Result<Self, RenameFailure> {
        self.rename_to(self, "")
    }

    /// Fails closed before mutation.
    pub fn remove_empty(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because identity cannot be compared.
    pub fn same_location(&self, _: &Self) -> Result<bool, FsError> {
        unsupported()
    }

    /// Fails closed because containment cannot be verified.
    pub fn contains_directory(&self, _: &Self) -> Result<bool, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed lock backend exists.
    pub fn lock(&self) -> Result<DirectoryLock, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed lock backend exists.
    pub fn lock_shared(&self) -> Result<DirectoryLock, FsError> {
        unsupported()
    }

    /// Fails closed because entries cannot be captured securely.
    pub fn entries(&self, _: usize) -> Result<Vec<std::ffi::OsString>, FsError> {
        unsupported()
    }

    /// Fails closed because containment cannot be verified.
    pub fn is_within(&self, _: &Self) -> Result<bool, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed snapshot backend exists.
    pub fn read_snapshot(&self, _: &Path, _: usize) -> Result<FileSnapshot, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn create_snapshot(&self, _: &OsStr, _: &[u8]) -> Result<FileSnapshot, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed observation backend exists.
    pub fn observe(&self, _: &str) -> Result<FileObservation, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed tree backend exists.
    pub fn capture(
        &self,
        _: TreeLimits,
        _: impl Fn(&str) -> bool,
    ) -> Result<CapturedTree, FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed tree backend exists.
    pub fn capture_limited(
        &self,
        _: TreeLimits,
        _: impl Fn(&str) -> Option<usize>,
    ) -> Result<CapturedTree, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn create_stream(
        &self,
        _: &str,
        _: usize,
        _: impl FnOnce(&mut BoundedWriter<'_>) -> Result<(), FsError>,
    ) -> Result<OwnedFile, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn write_tree(&self, _: &CapturedTree) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn write_tree_with(
        &self,
        _: &CapturedTree,
        _: impl FnMut() -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn remove_tree(
        &self,
        _: &CapturedTree,
        _: &mut impl FnMut() -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        unsupported()
    }
}

impl DirectoryLock {
    /// Fails closed because lock identity cannot be verified.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }
}

impl FileSnapshot {
    /// Returns no bytes; instances cannot be constructed safely.
    pub fn bytes(&self) -> &[u8] {
        &[]
    }

    /// Returns no identity; instances cannot be constructed safely.
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(0, 0)
    }

    /// Fails closed because snapshot identity cannot be verified.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because snapshot placement cannot be verified.
    pub fn verify_at(&self, _: &Directory, _: &str) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed and reports that no move happened.
    pub fn move_to(&self, _: &Directory, _: &OsStr) -> Result<Self, SnapshotMoveFailure> {
        Err(SnapshotMoveFailure {
            error: FsError::Unsupported,
            moved: None,
        })
    }

    #[doc(hidden)]
    pub fn move_with_hook(
        &self,
        parent: &Directory,
        name: &OsStr,
        _: impl FnOnce() -> Result<(), FsError>,
    ) -> Result<Self, SnapshotMoveFailure> {
        self.move_to(parent, name)
    }

    /// Fails closed before mutation.
    pub fn remove(&self) -> Result<(), FsError> {
        unsupported()
    }
}

impl ObservedFile {
    /// Returns no identity; instances cannot be constructed safely.
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(0, 0)
    }

    /// Returns no byte count; instances cannot be constructed safely.
    pub fn bytes(&self) -> u64 {
        0
    }

    /// Fails closed because observation identity cannot be verified.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed because no reviewed snapshot backend exists.
    pub fn read(&self, _: usize) -> Result<FileSnapshot, FsError> {
        unsupported()
    }
}

impl FileObservation {
    /// Fails closed because observations cannot be constructed safely.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Returns false; observations cannot be constructed safely.
    pub fn exists(&self) -> bool {
        false
    }
}

impl OwnedFile {
    /// Fails closed because file identity cannot be verified.
    pub fn verify(&self) -> Result<(), FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn remove(&self) -> Result<(), FsError> {
        unsupported()
    }
}

impl TemporaryFile {
    /// Fails closed before creating a temporary file.
    pub fn create(_: &str, _: &[u8]) -> Result<Self, FsError> {
        unsupported()
    }

    /// Fails closed before configuring a process argument.
    pub fn push_argument<'a>(
        &self,
        _: &'a mut std::process::Command,
    ) -> Result<&'a mut std::process::Command, FsError> {
        unsupported()
    }

    /// Fails closed before mutation.
    pub fn remove(&mut self) -> Result<(), FsError> {
        unsupported()
    }
}

fn unsupported<T>() -> Result<T, FsError> {
    Err(FsError::Unsupported)
}
