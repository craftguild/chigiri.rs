//! Capture a bounded tree once and verify identity between listing and completion.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;

use rustix::fs::{self, AtFlags, Dir, FileType, Stat};

use super::directory::single_name;
use super::file::same_contents;
use super::{CapturedTree, Directory, FsError, TreeLimits, os_error};
use crate::PortableRelativePath;

impl TreeLimits {
    fn validate(self) -> Result<(), FsError> {
        if self.depth > 64
            || self.entries > 65_536
            || self.files > 65_536
            || self.file_bytes > 64 * 1024 * 1024
            || self.total_bytes > 64 * 1024 * 1024
        {
            return Err(FsError::Limit);
        }
        Ok(())
    }
}

struct Observation {
    parent: Directory,
    name: String,
    stat: Stat,
}

impl Observation {
    fn verify(&self) -> Result<(), FsError> {
        let named = fs::statat(
            self.parent.descriptor(),
            self.name.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| FsError::Changed)?;
        if !same_contents(&self.stat, &named) {
            return Err(FsError::Changed);
        }
        Ok(())
    }
}

impl Directory {
    /// Captures files selected from one bounded no-follow tree.
    pub fn capture(
        &self,
        limits: TreeLimits,
        selected: impl Fn(&str) -> bool,
    ) -> Result<CapturedTree, FsError> {
        self.capture_with_hook(limits, selected, |_| {})
    }

    #[doc(hidden)]
    pub fn capture_with_hook(
        &self,
        limits: TreeLimits,
        selected: impl Fn(&str) -> bool,
        after_read: impl FnMut(&str),
    ) -> Result<CapturedTree, FsError> {
        self.capture_rules(
            limits,
            |path| selected(path).then_some(limits.file_bytes),
            after_read,
        )
    }

    /// Captures selected files with a separate byte ceiling for each path.
    pub fn capture_limited(
        &self,
        limits: TreeLimits,
        file_limit: impl Fn(&str) -> Option<usize>,
    ) -> Result<CapturedTree, FsError> {
        self.capture_rules(limits, file_limit, |_| {})
    }

    fn capture_rules(
        &self,
        limits: TreeLimits,
        file_limit: impl Fn(&str) -> Option<usize>,
        mut after_read: impl FnMut(&str),
    ) -> Result<CapturedTree, FsError> {
        limits.validate()?;
        self.verify()?;
        let mut result = CapturedTree {
            files: BTreeMap::new(),
            directories: BTreeSet::new(),
        };
        let mut pending = vec![(self.clone(), String::new(), 0)];
        let mut directories = Vec::new();
        let mut observations = Vec::new();
        let mut total_entries = 0;
        let mut total_bytes = 0;
        while let Some((directory, prefix, depth)) = pending.pop() {
            if depth > limits.depth {
                return Err(FsError::Limit);
            }
            let before = fs::fstat(directory.descriptor()).map_err(os_error)?;
            let entries = directory.captured_entries(&mut total_entries, limits.entries)?;
            for (name, stat) in entries {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let portable = PortableRelativePath::new(&path)?;
                observations.push(Observation {
                    parent: directory.clone(),
                    name: name.clone(),
                    stat,
                });
                match FileType::from_raw_mode(stat.st_mode) {
                    FileType::Directory => {
                        let child = directory.child(OsStr::new(&name))?;
                        result.directories.insert(portable.clone());
                        pending.push((child, path, depth + 1));
                    }
                    FileType::RegularFile if file_limit(&path).is_some() => {
                        if result.files.len() >= limits.files {
                            return Err(FsError::Limit);
                        }
                        let remaining = limits.total_bytes - total_bytes;
                        let (bytes, actual) = directory.read_observed(
                            OsStr::new(&name),
                            limits
                                .file_bytes
                                .min(remaining)
                                .min(file_limit(&path).unwrap_or(0)),
                            || {},
                        )?;
                        if !same_contents(&stat, &actual) {
                            return Err(FsError::Changed);
                        }
                        total_bytes += bytes.len();
                        if result.files.insert(portable, bytes).is_some() {
                            return Err(FsError::Changed);
                        }
                        after_read(&path);
                    }
                    FileType::RegularFile => {}
                    _ => return Err(FsError::Path),
                }
            }
            directory.verify()?;
            if !same_contents(
                &before,
                &fs::fstat(directory.descriptor()).map_err(os_error)?,
            ) {
                return Err(FsError::Changed);
            }
            directories.push((directory, before));
        }
        for observation in observations {
            observation.verify()?;
        }
        for (directory, before) in directories {
            directory.verify()?;
            if !same_contents(
                &before,
                &fs::fstat(directory.descriptor()).map_err(os_error)?,
            ) {
                return Err(FsError::Changed);
            }
        }
        Ok(result)
    }

    fn captured_entries(
        &self,
        total: &mut usize,
        maximum: usize,
    ) -> Result<Vec<(String, Stat)>, FsError> {
        let mut entries = BTreeMap::new();
        for entry in Dir::read_from(self.descriptor()).map_err(os_error)? {
            let entry = entry.map_err(os_error)?;
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            if *total >= maximum {
                return Err(FsError::Limit);
            }
            *total += 1;
            let name = std::str::from_utf8(bytes).map_err(|_| FsError::Path)?;
            single_name(OsStr::new(name))?;
            let stat =
                fs::statat(self.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW).map_err(os_error)?;
            if entries.insert(name.to_owned(), stat).is_some() {
                return Err(FsError::Changed);
            }
        }
        Ok(entries.into_iter().collect())
    }
}
