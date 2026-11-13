//! Retained file snapshots for verified project reads and no-replace movement.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags, Stat};

use super::directory::single_name;
use super::file::same_contents;
use super::{Directory, FsError, os_error};
use crate::filesystem::FileIdentity;

#[derive(Clone)]
/// Bounded immutable bytes with retained file and parent identity.
pub struct FileSnapshot {
    parent: Directory,
    name: OsString,
    bytes: Arc<[u8]>,
    recorded: Stat,
    file: Arc<File>,
}

/// Failure from a no-replace snapshot move with commit-point evidence.
pub struct SnapshotMoveFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Retained destination snapshot when the move crossed its commit point.
    pub moved: Option<Box<FileSnapshot>>,
}

impl Directory {
    /// Reads a bounded native path into an identity-retaining snapshot.
    pub fn read_snapshot(&self, path: &Path, maximum: usize) -> Result<FileSnapshot, FsError> {
        let name = path.file_name().ok_or(FsError::Path)?;
        let parent = path.parent().ok_or(FsError::Path)?;
        self.walk_native(parent)?.read_snapshot_name(name, maximum)
    }

    pub(super) fn read_snapshot_name(
        &self,
        name: &OsStr,
        maximum: usize,
    ) -> Result<FileSnapshot, FsError> {
        if maximum > 64 * 1024 * 1024 {
            return Err(FsError::Limit);
        }
        single_name(name)?;
        self.verify()?;
        let descriptor = fs::openat(
            self.descriptor(),
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os_error)?;
        let before = fs::fstat(&descriptor).map_err(os_error)?;
        if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
            return Err(FsError::Path);
        }
        if usize::try_from(before.st_size).map_err(|_| FsError::Limit)? > maximum {
            return Err(FsError::Limit);
        }
        let mut file = File::from(descriptor);
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| FsError::Io)?;
        if bytes.len() > maximum {
            return Err(FsError::Limit);
        }
        let after = fs::fstat(&file).map_err(os_error)?;
        let named = fs::statat(self.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| FsError::Changed)?;
        self.verify()?;
        if !same_contents(&before, &after)
            || !same_contents(&after, &named)
            || bytes.len() as u64 != after.st_size as u64
        {
            return Err(FsError::Changed);
        }
        Ok(FileSnapshot {
            parent: self.clone(),
            name: name.to_owned(),
            bytes: bytes.into(),
            recorded: after,
            file: Arc::new(file),
        })
    }

    /// Exclusively creates a direct-child file and retains its snapshot.
    pub fn create_snapshot(&self, name: &OsStr, bytes: &[u8]) -> Result<FileSnapshot, FsError> {
        single_name(name)?;
        self.verify()?;
        let descriptor = fs::openat(
            self.descriptor(),
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(os_error)?;
        let mut file = File::from(descriptor);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| FsError::Io)?;
        let recorded = fs::fstat(&file).map_err(os_error)?;
        let snapshot = FileSnapshot {
            parent: self.clone(),
            name: name.to_owned(),
            bytes: bytes.into(),
            recorded,
            file: Arc::new(file),
        };
        snapshot.verify()?;
        Ok(snapshot)
    }
}

impl FileSnapshot {
    /// Returns the retained immutable bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the opaque retained file identity.
    #[allow(clippy::unnecessary_cast)]
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(self.recorded.st_dev as u64, self.recorded.st_ino as u64)
    }

    /// Confirms the named file, descriptor, metadata, and bytes remain unchanged.
    pub fn verify(&self) -> Result<(), FsError> {
        self.parent.verify()?;
        let actual = fs::fstat(&self.file).map_err(os_error)?;
        let named = self
            .parent
            .read_snapshot_name(&self.name, self.bytes.len())?;
        if !same_contents(&self.recorded, &actual)
            || !same_contents(&self.recorded, &named.recorded)
            || self.bytes.as_ref() != named.bytes.as_ref()
        {
            return Err(FsError::Changed);
        }
        Ok(())
    }

    /// Confirms this snapshot still occupies a portable path below `root`.
    pub fn verify_at(&self, root: &Directory, path: &str) -> Result<(), FsError> {
        let actual = root.read_snapshot(Path::new(path), self.bytes.len())?;
        if !same_contents(&self.recorded, &actual.recorded)
            || self.bytes.as_ref() != actual.bytes.as_ref()
        {
            return Err(FsError::Changed);
        }
        Ok(())
    }

    /// Moves this snapshot to an absent direct-child name without replacement.
    pub fn move_to(&self, parent: &Directory, name: &OsStr) -> Result<Self, SnapshotMoveFailure> {
        self.move_with_hook(parent, name, || Ok(()))
    }

    #[doc(hidden)]
    pub fn move_with_hook(
        &self,
        parent: &Directory,
        name: &OsStr,
        after_rename: impl FnOnce() -> Result<(), FsError>,
    ) -> Result<Self, SnapshotMoveFailure> {
        let prepare = || {
            single_name(name)?;
            self.verify()?;
            parent.absent_os(name)
        };
        prepare().map_err(|error| SnapshotMoveFailure { error, moved: None })?;
        let mut moved = Self {
            parent: parent.clone(),
            name: name.to_owned(),
            ..self.clone()
        };
        fs::renameat_with(
            self.parent.descriptor(),
            &self.name,
            parent.descriptor(),
            name,
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| SnapshotMoveFailure {
            error: os_error(error),
            moved: None,
        })?;
        let result = (|| {
            moved.recorded = fs::fstat(&moved.file).map_err(os_error)?;
            if moved.identity() != self.identity() || moved.bytes.len() != self.bytes.len() {
                return Err(FsError::Changed);
            }
            after_rename()?;
            moved.verify()
        })();
        match result {
            Ok(()) => Ok(moved),
            Err(error) => Err(SnapshotMoveFailure {
                error,
                moved: Some(Box::new(moved)),
            }),
        }
    }

    /// Atomically claims, verifies, and removes this snapshot's named file.
    pub fn remove(&self) -> Result<(), FsError> {
        self.remove_with_hooks(|| {}, || {})
    }

    #[doc(hidden)]
    pub fn remove_with_hook(&self, after_verify: impl FnOnce()) -> Result<(), FsError> {
        self.remove_with_hooks(after_verify, || {})
    }

    #[doc(hidden)]
    pub fn remove_with_hooks(
        &self,
        after_verify: impl FnOnce(),
        after_claim: impl FnOnce(),
    ) -> Result<(), FsError> {
        self.verify()?;
        after_verify();
        super::remove::file(&self.parent, &self.name, &self.recorded, after_claim)
    }
}
