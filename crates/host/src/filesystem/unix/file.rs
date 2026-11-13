//! File-kind verification, bounded reads, and change detection through one descriptor.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};

use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, Stat};

use super::directory::{Identity, single_name};
use super::{Directory, FsError, os_error};

impl Directory {
    /// Reads a bounded regular file through a portable logical path.
    pub fn read(&self, path: &str, maximum: usize) -> Result<Vec<u8>, FsError> {
        let (parent, name) = self.parent_of(path)?;
        parent.read_with_hook(OsStr::new(&name), maximum, || {})
    }

    #[doc(hidden)]
    pub fn read_with_hook(
        &self,
        name: &OsStr,
        maximum: usize,
        after_open: impl FnOnce(),
    ) -> Result<Vec<u8>, FsError> {
        self.read_observed(name, maximum, after_open)
            .map(|(bytes, _)| bytes)
    }

    pub(super) fn read_observed(
        &self,
        name: &OsStr,
        maximum: usize,
        after_open: impl FnOnce(),
    ) -> Result<(Vec<u8>, Stat), FsError> {
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
        after_open();
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
        Ok((bytes, after))
    }

    /// Exclusively creates one bounded direct-child regular file.
    pub fn create(&self, name: &str, bytes: &[u8], maximum: usize) -> Result<(), FsError> {
        if bytes.len() > maximum {
            return Err(FsError::Limit);
        }
        single_name(OsStr::new(name))?;
        self.verify()?;
        let descriptor = fs::openat(
            self.descriptor(),
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(os_error)?;
        let mut file = File::from(descriptor);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| FsError::Io)?;
        let actual = fs::fstat(&file).map_err(os_error)?;
        let named = fs::statat(self.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| FsError::Changed)?;
        if !same_contents(&actual, &named) {
            return Err(FsError::Changed);
        }
        self.verify()?;
        self.sync()
    }
}

pub(super) fn same_contents(left: &Stat, right: &Stat) -> bool {
    Identity::from_stat(left) == Identity::from_stat(right)
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}
