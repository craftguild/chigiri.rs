//! Private random temporary files with retained creation and removal identity.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::remove::random_name;
use super::{Directory, FileSnapshot, FsError};
use crate::PortableRelativePath;

#[cfg(target_os = "linux")]
const TEMPORARY_ROOT: &str = "/tmp";
#[cfg(target_os = "macos")]
const TEMPORARY_ROOT: &str = "/private/tmp";

/// Random private temporary directory and its identity-retaining regular file.
pub struct TemporaryFile {
    directory: Option<Directory>,
    snapshot: Option<FileSnapshot>,
    name: PortableRelativePath,
}

impl TemporaryFile {
    /// Creates one private temporary directory and a fixed direct-child file.
    pub fn create(name: &str, bytes: &[u8]) -> Result<Self, FsError> {
        let name = PortableRelativePath::new(name)?;
        if name.as_str().contains('/') {
            return Err(FsError::Path);
        }
        let parent = Directory::open(Path::new(TEMPORARY_ROOT))?;
        for _ in 0..32 {
            let private_name = random_name(".chigiri-temporary-")?;
            let directory = match parent.create_directory_os(&private_name) {
                Ok(directory) => directory,
                Err(FsError::Exists) => continue,
                Err(error) => return Err(error),
            };
            match directory.create_snapshot(OsStr::new(name.as_str()), bytes) {
                Ok(snapshot) => {
                    return Ok(Self {
                        directory: Some(directory),
                        snapshot: Some(snapshot),
                        name,
                    });
                }
                Err(error) => {
                    let _ = directory.remove_empty();
                    return Err(error);
                }
            }
        }
        Err(FsError::Busy)
    }

    /// Adds the retained file path as a process argument without exposing it.
    pub fn push_argument<'a>(&self, command: &'a mut Command) -> Result<&'a mut Command, FsError> {
        self.snapshot.as_ref().ok_or(FsError::Changed)?.verify()?;
        self.directory
            .as_ref()
            .ok_or(FsError::Changed)?
            .push_descendant_argument(command, &self.name)
    }

    /// Removes only the retained file identity and then its private directory.
    pub fn remove(&mut self) -> Result<(), FsError> {
        if let Some(snapshot) = self.snapshot.as_ref() {
            snapshot.remove()?;
            self.snapshot = None;
        }
        if let Some(directory) = self.directory.as_ref() {
            directory.remove_empty()?;
            self.directory = None;
        }
        Ok(())
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}
