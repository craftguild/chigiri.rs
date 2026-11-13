//! Mock project policy over the shared no-follow host filesystem boundary.

use std::path::Path;

use chigiri_host::filesystem::{FileIdentity, FsError};

use super::ProjectError;

mod coordination;
mod mutation;
mod observation;
pub(super) use coordination::{DirectoryLock, TRANSACTION};
pub(super) use observation::Observation;

#[derive(Clone)]
pub(super) struct Directory(chigiri_host::filesystem::Directory);

impl Directory {
    pub fn output_parent(path: &Path) -> Result<Self, ProjectError> {
        Self::open(path).map_err(|mut error| {
            error.set_file("output");
            if error.message == "required project input missing" {
                error.exit = 2;
                error.phase = "environment";
                error.message = "cannot open output parent directory";
            }
            error
        })
    }

    pub fn open(path: &Path) -> Result<Self, ProjectError> {
        chigiri_host::filesystem::Directory::open(path)
            .map(Self)
            .map_err(|error| map(error, "path depth limit"))
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        self.0.verify().map_err(common)
    }

    pub fn read(&self, path: &Path, limit: usize) -> Result<FileSnapshot, ProjectError> {
        let snapshot = self
            .0
            .read_snapshot(path, limit)
            .map_err(|error| map(error, "input file byte limit"))?;
        let bytes = String::from_utf8(snapshot.bytes().to_vec())
            .map_err(|_| ProjectError::new("read", "UTF-8 input required"))?;
        Ok(Snapshot { snapshot, bytes })
    }

    pub fn identity(&self) -> FileIdentity {
        self.0.identity()
    }

    pub fn reference_from_child_to_path(
        &self,
        child: &std::ffi::OsStr,
        target_base: &Self,
        target: &Path,
    ) -> Result<String, ProjectError> {
        self.0
            .reference_from_child_to_path(child, &target_base.0, target)
            .map_err(common)
    }
}

#[derive(Clone)]
pub(super) struct Snapshot<B> {
    snapshot: chigiri_host::filesystem::FileSnapshot,
    pub bytes: B,
}

pub(super) type FileSnapshot = Snapshot<String>;
pub(super) type RawSnapshot = Snapshot<Vec<u8>>;

impl FileSnapshot {
    pub fn raw(&self) -> RawSnapshot {
        Snapshot {
            snapshot: self.snapshot.clone(),
            bytes: self.bytes.as_bytes().to_vec(),
        }
    }
}

impl<B: AsRef<[u8]>> Snapshot<B> {
    pub fn identity(&self) -> FileIdentity {
        self.snapshot.identity()
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        self.snapshot.verify().map_err(common)?;
        if self.bytes.as_ref() != self.snapshot.bytes() {
            return Err(changed());
        }
        Ok(())
    }
}

fn common(error: FsError) -> ProjectError {
    map(error, "input file byte limit")
}

fn map(error: FsError, limit: &'static str) -> ProjectError {
    match error {
        FsError::Exists => ProjectError::new("ownership", "exclusive destination already exists"),
        FsError::Missing => ProjectError::new("read", "required project input missing"),
        FsError::Busy => ProjectError::new("ownership", "project is being updated"),
        FsError::Changed => changed(),
        FsError::Path => invalid(),
        FsError::Limit => ProjectError::new("limit", limit),
        FsError::Unsupported => {
            let mut error = ProjectError::new("environment", "secure host capability unavailable");
            error.exit = 2;
            error
        }
        FsError::Cancelled => ProjectError::new("cancel", "project operation cancelled"),
        FsError::Io => io_error(),
        _ => io_error(),
    }
}

fn changed() -> ProjectError {
    ProjectError::new("ownership", "input changed during command")
}

fn invalid() -> ProjectError {
    ProjectError::new("read", "regular no-follow file required")
}

fn io_error() -> ProjectError {
    let mut error = ProjectError::new("read", "cannot read project input");
    error.exit = 2;
    error
}
