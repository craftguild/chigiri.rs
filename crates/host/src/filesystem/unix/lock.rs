//! Excludes concurrent use of an output name under the same parent inode; the OS releases the lock on process exit.

use std::os::fd::OwnedFd;

use rustix::fs::{self, FlockOperation, Mode, OFlags};

use super::directory::Identity;
use super::{Directory, FsError, os_error};

/// Excluding non-blocking, which is only valid for the duration of descriptor possession.
pub struct DirectoryLock {
    directory: Directory,
    descriptor: OwnedFd,
}

impl Directory {
    /// Acquires a non-blocking exclusive lock tied to this directory identity.
    pub fn lock(&self) -> Result<DirectoryLock, FsError> {
        self.lock_with(FlockOperation::NonBlockingLockExclusive)
    }

    /// Acquires a non-blocking shared lock tied to this directory identity.
    pub fn lock_shared(&self) -> Result<DirectoryLock, FsError> {
        self.lock_with(FlockOperation::NonBlockingLockShared)
    }

    fn lock_with(&self, operation: FlockOperation) -> Result<DirectoryLock, FsError> {
        self.verify()?;
        // A duplicated descriptor cannot detect lock contention within the same process.
        let descriptor = fs::openat(
            self.descriptor(),
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os_error)?;
        fs::flock(&descriptor, operation).map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                FsError::Busy
            } else {
                os_error(error)
            }
        })?;
        let lock = DirectoryLock {
            directory: self.clone(),
            descriptor,
        };
        lock.verify()?;
        Ok(lock)
    }
}

impl DirectoryLock {
    /// Confirms the lock and its directory identity remain valid.
    pub fn verify(&self) -> Result<(), FsError> {
        self.directory.verify()?;
        let locked = fs::fstat(&self.descriptor).map_err(os_error)?;
        let current = fs::fstat(self.directory.descriptor()).map_err(os_error)?;
        if Identity::from_stat(&locked) != Identity::from_stat(&current) {
            return Err(FsError::Changed);
        }
        Ok(())
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // Forks in parallel spawn may temporarily inherit the same open file description.
        // Release explicitly when the owner finishes instead of relying only on CLOEXEC/close.
        for _ in 0..8 {
            match fs::flock(&self.descriptor, FlockOperation::NonBlockingUnlock) {
                Err(rustix::io::Errno::INTR) => {}
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::Directory;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn lock_release_does_not_depend_on_inherited_descriptor_close() {
        let path = std::env::temp_dir().join(format!(
            "chigiri-host-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let path = std::fs::canonicalize(path).unwrap();
        let directory = Directory::open(&path).unwrap();
        let lock = directory.lock().unwrap();
        let inherited = lock.descriptor.try_clone().unwrap();
        drop(lock);
        let next = directory.lock().unwrap();
        next.verify().unwrap();
        drop(inherited);
        drop(next);
        std::fs::remove_dir(path).unwrap();
    }
}
