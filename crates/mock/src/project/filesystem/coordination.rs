//! Project locking and unfinished-transaction policy.

use std::ffi::OsStr;

use super::{Directory, ProjectError, changed, common};

pub(in crate::project) const TRANSACTION: &str = ".chigiri-mock-transaction";

pub(in crate::project) struct DirectoryLock(chigiri_host::filesystem::DirectoryLock);

impl Directory {
    pub fn is_within(&self, root: &Directory) -> Result<bool, ProjectError> {
        self.0.is_within(&root.0).map_err(common)
    }

    pub fn lock(&self, exclusive: bool) -> Result<DirectoryLock, ProjectError> {
        let lock = if exclusive {
            self.0.lock()
        } else {
            self.0.lock_shared()
        };
        lock.map(DirectoryLock).map_err(common)
    }

    pub fn transaction_absent(&self) -> Result<(), ProjectError> {
        match self.0.absent_os(OsStr::new(TRANSACTION)) {
            Ok(()) => Ok(()),
            Err(chigiri_host::filesystem::FsError::Exists) => Err(ProjectError::new(
                "ownership",
                "unfinished project transaction",
            )),
            Err(error) => Err(common(error)),
        }
    }
}

impl DirectoryLock {
    pub fn verify(&self) -> Result<(), ProjectError> {
        self.0.verify().map_err(|error| {
            if error == chigiri_host::filesystem::FsError::Changed {
                changed()
            } else {
                common(error)
            }
        })
    }
}
