//! Retains ancestor descriptors to verify directory names and identity.

mod authority;
pub(super) mod mutation;
mod overlap;

use std::ffi::{OsStr, OsString};
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, Stat};

use super::{FsError, os_error, relative_components};
use crate::filesystem::FileIdentity;

#[derive(Clone)]
/// Retained no-follow directory capability with ancestor identity.
pub struct Directory(Arc<Handle>);

struct Handle {
    fd: OwnedFd,
    parent: Option<(Directory, OsString)>,
    identity: Identity,
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
    kind: FileType,
}

impl Identity {
    // Linux and macOS use different stat integer types; normalize them for comparison.
    #[allow(clippy::unnecessary_cast)]
    pub(super) fn from_stat(stat: &Stat) -> Self {
        Self {
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
            kind: FileType::from_raw_mode(stat.st_mode),
        }
    }
}

impl std::fmt::Debug for Directory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Directory { .. }")
    }
}

impl Directory {
    /// Opens a native user path component by component without following links.
    pub fn open(path: &Path) -> Result<Self, FsError> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir().map_err(|_| FsError::Io)?.join(path)
        };
        let text = absolute.to_str().ok_or(FsError::Path)?;
        if text.len() > 4096 || text.contains(['\\', ':']) || text.chars().any(char::is_control) {
            return Err(FsError::Path);
        }
        let fd = fs::open("/", directory_flags(), Mode::empty()).map_err(os_error)?;
        let stat = fs::fstat(&fd).map_err(os_error)?;
        let mut directory = Self(Arc::new(Handle {
            fd,
            parent: None,
            identity: Identity::from_stat(&stat),
            path: PathBuf::from("/"),
        }));
        let mut depth = 0;
        for component in absolute.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => {
                    depth += 1;
                    if depth > 64 {
                        return Err(FsError::Limit);
                    }
                    directory = directory.child(name)?;
                }
                Component::ParentDir => {
                    directory.verify()?;
                    directory = directory
                        .0
                        .parent
                        .as_ref()
                        .map_or_else(|| directory.clone(), |(parent, _)| parent.clone());
                }
                _ => return Err(FsError::Path),
            }
        }
        Ok(directory)
    }

    pub(super) fn descriptor(&self) -> &OwnedFd {
        &self.0.fd
    }

    /// Returns the opaque identity captured when this directory was opened.
    pub fn identity(&self) -> FileIdentity {
        FileIdentity::new(self.0.identity.device, self.0.identity.inode)
    }

    /// Confirms the descriptor and every retained ancestor still have their names.
    pub fn verify(&self) -> Result<(), FsError> {
        let mut current = self;
        loop {
            let stat = fs::fstat(current.descriptor()).map_err(os_error)?;
            if Identity::from_stat(&stat) != current.0.identity {
                return Err(FsError::Changed);
            }
            let Some((parent, name)) = &current.0.parent else {
                return Ok(());
            };
            current.verify_entry(parent, name)?;
            current = parent;
        }
    }

    fn verify_entry(&self, parent: &Self, name: &OsStr) -> Result<(), FsError> {
        let stat = fs::statat(parent.descriptor(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| FsError::Changed)?;
        if Identity::from_stat(&stat) != self.0.identity {
            return Err(FsError::Changed);
        }
        Ok(())
    }

    /// Opens one direct child directory without following links.
    pub fn child(&self, name: &OsStr) -> Result<Self, FsError> {
        single_name(name)?;
        self.verify()?;
        let fd = fs::openat(self.descriptor(), name, directory_flags(), Mode::empty())
            .map_err(os_error)?;
        let stat = fs::fstat(&fd).map_err(os_error)?;
        let child = Self(Arc::new(Handle {
            fd,
            parent: Some((self.clone(), name.to_owned())),
            identity: Identity::from_stat(&stat),
            path: self.0.path.join(name),
        }));
        child.verify()?;
        Ok(child)
    }

    /// Opens a validated portable descendant directory.
    pub fn relative(&self, path: &str) -> Result<Self, FsError> {
        let mut directory = self.clone();
        for name in relative_components(path)? {
            directory = directory.child(OsStr::new(name))?;
        }
        Ok(directory)
    }

    pub(super) fn walk_native(&self, path: &Path) -> Result<Self, FsError> {
        let mut directory = if path.is_absolute() {
            Self::open(Path::new("/"))?
        } else {
            self.clone()
        };
        let mut depth = 0usize;
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    directory.verify()?;
                    if let Some((parent, _)) = &directory.0.parent {
                        directory = parent.clone();
                    }
                }
                Component::Normal(name) => {
                    depth += 1;
                    if depth > 64 {
                        return Err(FsError::Limit);
                    }
                    directory = directory.child(name)?;
                }
                _ => return Err(FsError::Path),
            }
        }
        directory.verify()?;
        Ok(directory)
    }

    /// Opens the parent capability and final component of a portable path.
    pub fn parent_of(&self, path: &str) -> Result<(Self, String), FsError> {
        let mut components = relative_components(path)?;
        let name = components.pop().ok_or(FsError::Path)?.to_owned();
        let mut directory = self.clone();
        for name in components {
            directory = directory.child(OsStr::new(name))?;
        }
        Ok((directory, name))
    }

    /// Synchronizes directory metadata to stable storage.
    pub fn sync(&self) -> Result<(), FsError> {
        fs::fsync(self.descriptor()).map_err(os_error)
    }
}

pub(super) fn single_name(name: &OsStr) -> Result<(), FsError> {
    let name = name.to_str().ok_or(FsError::Path)?;
    if relative_components(name)?.len() != 1 {
        return Err(FsError::Path);
    }
    Ok(())
}

fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}
