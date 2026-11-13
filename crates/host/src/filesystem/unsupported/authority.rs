//! Fail-closed opaque native-path operations for unreviewed hosts.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::Directory;
use crate::PortableRelativePath;
use crate::filesystem::{FsError, ResolvedPath};

impl Directory {
    /// Fails closed because no reviewed destination backend exists.
    pub fn destination(&self) -> Result<ResolvedPath, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no reviewed destination backend exists.
    pub fn descendant_destination(
        &self,
        _: &PortableRelativePath,
    ) -> Result<ResolvedPath, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no reviewed directory resolver exists.
    pub fn resolve_directory(&self, _: &Path) -> Result<Self, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no reviewed file resolver exists.
    pub fn resolve_file_parent(&self, _: &Path) -> Result<(Self, String), FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no retained containment can be established.
    pub fn descendant_label(&self, _: &Self, _: &str) -> Result<PortableRelativePath, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no native emitted path can be inspected.
    pub fn emitted_label(&self, _: &Path) -> Result<PortableRelativePath, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed before configuring a process.
    pub fn set_current_dir<'a>(&self, _: &'a mut Command) -> Result<&'a mut Command, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed before configuring a process.
    pub fn push_argument<'a>(&self, _: &'a mut Command) -> Result<&'a mut Command, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed before configuring a process.
    pub fn push_descendant_argument<'a>(
        &self,
        _: &'a mut Command,
        _: &PortableRelativePath,
    ) -> Result<&'a mut Command, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no native reference can be derived.
    pub fn reference_from_descendant(
        &self,
        _: &PortableRelativePath,
        _: &Self,
    ) -> Result<String, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no native reference can be derived.
    pub fn reference_to(&self, _: &Self) -> Result<String, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no native reference can be derived.
    pub fn reference_from_child_to_path(
        &self,
        _: &OsStr,
        _: &Self,
        _: &Path,
    ) -> Result<String, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because overlap cannot be verified.
    pub fn overlaps_destination(
        &self,
        _: &ResolvedPath,
        _: &ResolvedPath,
    ) -> Result<bool, FsError> {
        Err(FsError::Unsupported)
    }

    /// Fails closed because no reviewed resolution backend exists.
    pub fn resolve_destination(&self, _: &Path) -> Result<ResolvedPath, FsError> {
        Err(FsError::Unsupported)
    }
}
