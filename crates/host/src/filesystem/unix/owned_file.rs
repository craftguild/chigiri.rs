//! Writes bounded recovery records and removes them through retained ownership
//! descriptors.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use rustix::fs::{self, AtFlags, Mode, OFlags, Stat};
use sha2::{Digest as _, Sha256};

use super::directory::single_name;
use super::file::same_contents;
use super::{Directory, FsError, os_error};

/// Exclusively created recovery file retained through final removal.
pub struct OwnedFile {
    parent: Directory,
    name: String,
    file: File,
    recorded: Stat,
}

/// Streaming writer that refuses bytes beyond its fixed ceiling.
pub struct BoundedWriter<'a> {
    file: &'a mut File,
    remaining: usize,
    digest: Sha256,
}

impl BoundedWriter<'_> {
    /// Writes one byte slice if it fits in the remaining budget.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), FsError> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or(FsError::Limit)?;
        self.file.write_all(bytes).map_err(|_| FsError::Io)?;
        self.digest.update(bytes);
        Ok(())
    }
}

impl Directory {
    /// Exclusively creates a bounded streaming recovery file.
    pub fn create_stream(
        &self,
        name: &str,
        maximum: usize,
        write: impl FnOnce(&mut BoundedWriter<'_>) -> Result<(), FsError>,
    ) -> Result<OwnedFile, FsError> {
        // The recovery archive allows 64 MiB of data plus up to 4 MiB of
        // bounded path and index metadata.
        if maximum > 68 * 1024 * 1024 {
            return Err(FsError::Limit);
        }
        single_name(std::ffi::OsStr::new(name))?;
        self.verify()?;
        let descriptor = fs::openat(
            self.descriptor(),
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(os_error)?;
        let recorded = fs::fstat(&descriptor).map_err(os_error)?;
        let mut owned = OwnedFile {
            parent: self.clone(),
            name: name.to_owned(),
            file: File::from(descriptor),
            recorded,
        };
        let mut writer = BoundedWriter {
            file: &mut owned.file,
            remaining: maximum,
            digest: Sha256::new(),
        };
        write(&mut writer)?;
        let written = maximum - writer.remaining;
        let digest: [u8; 32] = writer.digest.finalize().into();
        owned.file.sync_all().map_err(|_| FsError::Io)?;
        owned.recorded = fs::fstat(&owned.file).map_err(os_error)?;
        if owned.recorded.st_size as u64 != written as u64 {
            return Err(FsError::Changed);
        }
        owned.verify_bytes(written, digest)?;
        owned.verify()?;
        self.sync()?;
        Ok(owned)
    }
}

impl OwnedFile {
    fn verify_bytes(&mut self, length: usize, expected: [u8; 32]) -> Result<(), FsError> {
        self.verify()?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| FsError::Io)?;
        let mut digest = Sha256::new();
        let mut actual = 0usize;
        let mut buffer = [0u8; 8192];
        loop {
            let count = self.file.read(&mut buffer).map_err(|_| FsError::Io)?;
            if count == 0 {
                break;
            }
            actual = actual.checked_add(count).ok_or(FsError::Limit)?;
            if actual > length {
                return Err(FsError::Changed);
            }
            digest.update(&buffer[..count]);
        }
        let digest: [u8; 32] = digest.finalize().into();
        if actual != length || digest != expected {
            return Err(FsError::Changed);
        }
        self.verify()
    }

    /// Confirms the opened file still owns its original direct-child name.
    pub fn verify(&self) -> Result<(), FsError> {
        self.parent.verify()?;
        let actual = fs::fstat(&self.file).map_err(os_error)?;
        let named = fs::statat(
            self.parent.descriptor(),
            self.name.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| FsError::Changed)?;
        if !same_contents(&self.recorded, &actual) || !same_contents(&actual, &named) {
            return Err(FsError::Changed);
        }
        Ok(())
    }

    /// Removes a verified file; callers retain the primary failure and recovery record.
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
        super::remove::file(
            &self.parent,
            OsStr::new(&self.name),
            &self.recorded,
            after_claim,
        )
    }
}
