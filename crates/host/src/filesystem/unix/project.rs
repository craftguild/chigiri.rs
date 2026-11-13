//! Project-oriented observation composed from the common directory capability.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use rustix::fs::{self, AtFlags, Dir, FileType, Mode, OFlags, Stat};

use super::directory::single_name;
use super::file::same_contents;
use super::{Directory, FileSnapshot, FsError, os_error, relative_components};
use crate::filesystem::FileIdentity;

#[derive(Clone)]
/// Retained observation of either a regular file or an absent direct name.
pub enum FileObservation {
    /// A regular file was present and its identity is retained.
    Present(Arc<ObservedFile>),
    /// The parent identity and absent final name are retained.
    Absent(Directory, OsString),
}

/// Retained regular-file identity and byte count.
pub struct ObservedFile {
    parent: Directory,
    name: OsString,
    file: File,
    recorded: Stat,
}

/// Failure from exclusive directory creation with commit-point evidence.
pub struct DirectoryCreationFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Whether the directory was created before the failure.
    pub created: bool,
}

impl Directory {
    /// Confirms one native direct-child name is absent.
    pub fn absent_os(&self, name: &OsStr) -> Result<(), FsError> {
        single_name(name)?;
        self.verify()?;
        match fs::statat(self.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Ok(_) => Err(FsError::Exists),
            Err(error) => Err(os_error(error)),
        }
    }

    /// Opens an optional direct child directory without following links.
    pub fn optional_child(&self, name: &OsStr) -> Result<Option<Self>, FsError> {
        match self.absent_os(name) {
            Ok(()) => Ok(None),
            Err(FsError::Exists) => self.child(name).map(Some),
            Err(error) => Err(error),
        }
    }

    /// Exclusively creates a direct child using a native component.
    pub fn create_directory_os(&self, name: &OsStr) -> Result<Self, FsError> {
        let name = name.to_str().ok_or(FsError::Path)?;
        self.create_directory(name)
    }

    /// Creates a direct child and reports whether failure happened after creation.
    pub fn create_directory_observed(
        &self,
        name: &OsStr,
    ) -> Result<Self, DirectoryCreationFailure> {
        let create = || {
            single_name(name)?;
            self.verify()?;
            fs::mkdirat(self.descriptor(), name, Mode::RWXU).map_err(os_error)
        };
        create().map_err(|error| DirectoryCreationFailure {
            error,
            created: false,
        })?;
        self.child(name).map_err(|error| DirectoryCreationFailure {
            error,
            created: true,
        })
    }

    /// Lists at most `maximum` direct child names without following them.
    pub fn entries(&self, maximum: usize) -> Result<Vec<OsString>, FsError> {
        self.verify()?;
        let mut result = Vec::new();
        for entry in Dir::read_from(self.descriptor()).map_err(os_error)? {
            let entry = entry.map_err(os_error)?;
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            if result.len() == maximum {
                return Err(FsError::Limit);
            }
            let name = std::str::from_utf8(bytes).map_err(|_| FsError::Path)?;
            single_name(OsStr::new(name))?;
            result.push(name.into());
        }
        self.verify()?;
        Ok(result)
    }

    /// Reports whether this directory is within `root` by retained identity.
    pub fn is_within(&self, root: &Self) -> Result<bool, FsError> {
        root.contains_directory(self)
    }

    /// Observes one portable logical file path or its absence.
    pub fn observe(&self, path: &str) -> Result<FileObservation, FsError> {
        let parts = relative_components(path)?;
        let mut parent = self.clone();
        for (index, part) in parts.iter().enumerate() {
            let name = OsStr::new(part);
            parent.verify()?;
            match fs::statat(parent.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => {
                    return Ok(FileObservation::Absent(parent, name.to_owned()));
                }
                Err(error) => return Err(os_error(error)),
                Ok(_) if index + 1 < parts.len() => parent = parent.child(name)?,
                Ok(_) => {
                    return ObservedFile::open(parent, name)
                        .map(Arc::new)
                        .map(FileObservation::Present);
                }
            }
        }
        Err(FsError::Path)
    }
}

impl ObservedFile {
    fn open(parent: Directory, name: &OsStr) -> Result<Self, FsError> {
        let descriptor = fs::openat(
            parent.descriptor(),
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os_error)?;
        let recorded = fs::fstat(&descriptor).map_err(os_error)?;
        if FileType::from_raw_mode(recorded.st_mode) != FileType::RegularFile {
            return Err(FsError::Path);
        }
        let observed = Self {
            parent,
            name: name.to_owned(),
            file: File::from(descriptor),
            recorded,
        };
        observed.verify()?;
        Ok(observed)
    }

    /// Returns the opaque observed file identity.
    #[allow(clippy::unnecessary_cast)]
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(self.recorded.st_dev as u64, self.recorded.st_ino as u64)
    }

    /// Returns the observed byte count.
    pub fn bytes(&self) -> u64 {
        self.recorded.st_size as u64
    }

    /// Confirms the observed name still identifies the same file state.
    pub fn verify(&self) -> Result<(), FsError> {
        self.parent.verify()?;
        let actual = fs::fstat(&self.file).map_err(os_error)?;
        let named = fs::statat(
            self.parent.descriptor(),
            &self.name,
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(os_error)?;
        if !same_contents(&self.recorded, &actual) || !same_contents(&actual, &named) {
            return Err(FsError::Changed);
        }
        Ok(())
    }

    /// Reads and retains a bounded snapshot of the observed file.
    pub fn read(&self, maximum: usize) -> Result<FileSnapshot, FsError> {
        self.verify()?;
        let snapshot = self.parent.read_snapshot(Path::new(&self.name), maximum)?;
        self.verify()?;
        if self.identity() != snapshot.identity() {
            return Err(FsError::Changed);
        }
        Ok(snapshot)
    }
}

impl FileObservation {
    /// Confirms the present or absent observation remains true.
    pub fn verify(&self) -> Result<(), FsError> {
        match self {
            Self::Present(file) => file.verify(),
            Self::Absent(parent, name) => parent.absent_os(name),
        }
    }

    /// Reports whether this observation retained a present file.
    pub fn exists(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}
