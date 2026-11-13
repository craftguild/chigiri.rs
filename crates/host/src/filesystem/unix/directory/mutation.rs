//! Creates, renames, and deletes empty directories through retained handles.

use super::super::{FsError, os_error, relative_components};
use super::{Directory, Handle, single_name};
use crate::filesystem::ResolvedPath;
use rustix::fs::{self, AtFlags, Mode, RenameFlags};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Preserve the original path if rename verification or synchronization fails.
#[derive(Debug)]
pub struct RenameFailure {
    /// Closed failure classification.
    pub error: FsError,
    /// Retained destination handle when the rename crossed its commit point.
    pub moved: Option<Directory>,
}

impl From<FsError> for RenameFailure {
    fn from(error: FsError) -> Self {
        Self { error, moved: None }
    }
}

impl Directory {
    /// All existing components are verified by no-follow and not just created the missing end.
    pub fn resolve_destination(&self, path: &Path) -> Result<ResolvedPath, FsError> {
        self.resolve_with_ancestor(path)
            .map(|(path, _)| ResolvedPath::new(path))
    }

    pub(super) fn resolve_with_ancestor(&self, path: &Path) -> Result<(PathBuf, Self), FsError> {
        if path.as_os_str().len() > 4096 || path.components().count() > 65 {
            return Err(FsError::Limit);
        }
        let mut directory = if path.is_absolute() {
            Self::open(Path::new("/"))?
        } else {
            self.clone()
        };
        let mut missing = Vec::new();
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    if missing.pop().is_none() {
                        directory.verify()?;
                        if let Some((parent, _)) = &directory.0.parent {
                            directory = parent.clone();
                        }
                    }
                }
                Component::Normal(name) => {
                    single_name(name)?;
                    if missing.is_empty() {
                        match directory.child(name) {
                            Ok(child) => directory = child,
                            Err(FsError::Missing) => missing.push(name.to_owned()),
                            Err(error) => return Err(error),
                        }
                    } else {
                        missing.push(name.to_owned());
                    }
                }
                _ => return Err(FsError::Path),
            }
        }
        directory.verify()?;
        let mut result = directory.0.path.clone();
        result.extend(missing);
        if result.components().count() > 65 || result.as_os_str().len() > 4096 {
            return Err(FsError::Limit);
        }
        Ok((result, directory))
    }

    /// Confirms one direct portable name is absent.
    pub fn absent(&self, name: &str) -> Result<(), FsError> {
        single_name(OsStr::new(name))?;
        self.verify()?;
        match fs::statat(self.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => self.verify(),
            Ok(_) => Err(FsError::Exists),
            Err(error) => Err(os_error(error)),
        }
    }

    /// Exclusively creates and opens one direct portable directory.
    pub fn create_directory(&self, name: &str) -> Result<Self, FsError> {
        single_name(OsStr::new(name))?;
        self.verify()?;
        fs::mkdirat(self.descriptor(), name, Mode::RWXU).map_err(os_error)?;
        let created = self.child(OsStr::new(name))?;
        self.sync()?;
        created.verify()?;
        Ok(created)
    }

    /// Creates only missing intermediate directories and opens existing entries without following links.
    pub fn ensure_parent(&self, path: &str) -> Result<(Self, String), FsError> {
        let mut components = relative_components(path)?;
        let name = components.pop().ok_or(FsError::Path)?.to_owned();
        let mut directory = self.clone();
        for part in components {
            directory = match directory.child(OsStr::new(part)) {
                Ok(child) => child,
                Err(FsError::Missing) => match directory.create_directory(part) {
                    Ok(child) => child,
                    // Only the parent creation of a different generation can be combined in the same secure open.
                    Err(FsError::Exists) => directory.child(OsStr::new(part))?,
                    Err(error) => return Err(error),
                },
                Err(error) => return Err(error),
            };
        }
        Ok((directory, name))
    }

    /// Moves this directory without replacing an existing destination.
    pub fn rename_to(&self, parent: &Self, name: &str) -> Result<Self, RenameFailure> {
        self.rename_with_hook(parent, name, || Ok(()))
    }

    #[doc(hidden)]
    pub fn rename_with_hook(
        &self,
        parent: &Self,
        name: &str,
        after_rename: impl FnOnce() -> Result<(), FsError>,
    ) -> Result<Self, RenameFailure> {
        single_name(OsStr::new(name))?;
        self.verify()?;
        parent.verify()?;
        let (old_parent, old_name) = self.0.parent.as_ref().ok_or(FsError::Path)?;
        // Rename through the parent descriptor; no ownership transfer or post-rename reopen is required.
        let descriptor = self.descriptor().try_clone().map_err(|_| FsError::Io)?;
        let moved = Self(Arc::new(Handle {
            fd: descriptor,
            parent: Some((parent.clone(), name.into())),
            identity: self.0.identity,
            path: parent.0.path.join(name),
        }));
        fs::renameat_with(
            old_parent.descriptor(),
            old_name,
            parent.descriptor(),
            name,
            RenameFlags::NOREPLACE,
        )
        .map_err(os_error)?;
        let verified = after_rename()
            .and_then(|()| moved.verify())
            .and_then(|()| old_parent.verify())
            .and_then(|()| old_parent.sync())
            .and_then(|()| parent.sync());
        match verified {
            Ok(()) => Ok(moved),
            Err(error) => Err(RenameFailure {
                error,
                moved: Some(moved),
            }),
        }
    }

    /// Remove an empty directory without redirection or symlink traversal.
    /// The OS rejects non-empty directories.
    pub fn remove_empty(&self) -> Result<(), FsError> {
        self.remove_empty_with_hook(|| {})
    }

    #[doc(hidden)]
    pub fn remove_empty_with_hook(&self, after_verify: impl FnOnce()) -> Result<(), FsError> {
        self.verify()?;
        let (parent, name) = self.0.parent.as_ref().ok_or(FsError::Path)?;
        after_verify();
        super::super::remove::directory(parent, name, self.0.identity)
    }
}
