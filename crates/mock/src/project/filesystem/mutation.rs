//! Project mutation policy over exclusive host primitives.

use std::ffi::{OsStr, OsString};

use super::{Directory, FileSnapshot, ProjectError, Snapshot, common, invalid, map};

pub(in crate::project) struct MoveFailure<T> {
    pub error: Box<ProjectError>,
    pub moved: Option<Box<T>>,
}

pub(in crate::project) struct CreationFailure {
    pub error: Box<ProjectError>,
    pub created: bool,
}

impl Directory {
    pub fn absent(&self, name: &OsStr) -> Result<(), ProjectError> {
        self.0.absent_os(name).map_err(common)
    }

    pub fn child(&self, name: &OsStr) -> Result<Self, ProjectError> {
        self.0.child(name).map(Self).map_err(common)
    }

    pub fn create_directory(&self, name: &OsStr) -> Result<Self, ProjectError> {
        self.0.create_directory_os(name).map(Self).map_err(common)
    }

    pub fn create_directory_observed(&self, name: &OsStr) -> Result<Self, CreationFailure> {
        self.0
            .create_directory_observed(name)
            .map(Self)
            .map_err(|failure| CreationFailure {
                error: Box::new(common(failure.error)),
                created: failure.created,
            })
    }

    pub fn optional_child(&self, name: &OsStr) -> Result<Option<Self>, ProjectError> {
        self.0
            .optional_child(name)
            .map(|directory| directory.map(Self))
            .map_err(common)
    }

    pub fn create_file(&self, name: &OsStr, bytes: &str) -> Result<FileSnapshot, ProjectError> {
        let snapshot = self
            .0
            .create_snapshot(name, bytes.as_bytes())
            .map_err(common)?;
        Ok(Snapshot {
            snapshot,
            bytes: bytes.to_owned(),
        })
    }

    pub fn remove_empty(&self) -> Result<(), ProjectError> {
        self.0.remove_empty().map_err(common)
    }

    pub fn move_to(&self, parent: &Self, name: &OsStr) -> Result<Self, MoveFailure<Self>> {
        self.move_with(parent, name, || Ok(()))
    }

    pub(in crate::project) fn move_with(
        &self,
        parent: &Self,
        name: &OsStr,
        after_rename: impl FnOnce() -> Result<(), ProjectError>,
    ) -> Result<Self, MoveFailure<Self>> {
        let name = name.to_str().ok_or_else(|| MoveFailure {
            error: Box::new(invalid()),
            moved: None,
        })?;
        let mut product_error = None;
        let result = self.0.rename_with_hook(&parent.0, name, || {
            after_rename().map_err(|error| {
                product_error = Some(error);
                chigiri_host::filesystem::FsError::Io
            })
        });
        result.map(Self).map_err(|failure| MoveFailure {
            error: Box::new(
                product_error
                    .take()
                    .unwrap_or_else(|| common(failure.error)),
            ),
            moved: failure.moved.map(Self).map(Box::new),
        })
    }

    pub fn entries(&self, maximum: usize) -> Result<Vec<OsString>, ProjectError> {
        self.0
            .entries(maximum)
            .map_err(|error| map(error, "directory entry count limit"))
    }
}

impl<B: AsRef<[u8]> + Clone> Snapshot<B> {
    pub fn verify_at(&self, root: &Directory, path: &str) -> Result<(), ProjectError> {
        self.snapshot.verify_at(&root.0, path).map_err(common)?;
        if self.bytes.as_ref() != self.snapshot.bytes() {
            return Err(super::changed());
        }
        Ok(())
    }

    pub fn move_to(&self, parent: &Directory, name: &OsStr) -> Result<Self, MoveFailure<Self>> {
        self.move_with(parent, name, || Ok(()))
    }

    pub(in crate::project) fn move_with(
        &self,
        parent: &Directory,
        name: &OsStr,
        after_rename: impl FnOnce() -> Result<(), ProjectError>,
    ) -> Result<Self, MoveFailure<Self>> {
        let mut product_error = None;
        let result = self.snapshot.move_with_hook(&parent.0, name, || {
            after_rename().map_err(|error| {
                product_error = Some(error);
                chigiri_host::filesystem::FsError::Io
            })
        });
        result
            .map(|snapshot| Self {
                snapshot,
                bytes: self.bytes.clone(),
            })
            .map_err(|failure| MoveFailure {
                error: Box::new(
                    product_error
                        .take()
                        .unwrap_or_else(|| common(failure.error)),
                ),
                moved: failure
                    .moved
                    .map(|snapshot| Self {
                        snapshot: *snapshot,
                        bytes: self.bytes.clone(),
                    })
                    .map(Box::new),
            })
    }

    pub fn remove(&self) -> Result<(), ProjectError> {
        self.snapshot.remove().map_err(common)
    }
}
