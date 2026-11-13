//! Opaque native-path use derived from retained directory capabilities.

use std::ffi::OsStr;
use std::path::{Component, Path};
use std::process::Command;

use super::{Directory, single_name};
use crate::PortableRelativePath;
use crate::filesystem::{FsError, ResolvedPath};

impl Directory {
    /// Returns an opaque validated destination for this directory.
    pub fn destination(&self) -> Result<ResolvedPath, FsError> {
        self.verify()?;
        Ok(ResolvedPath::new(self.0.path.clone()))
    }

    /// Returns an opaque portable descendant destination.
    pub fn descendant_destination(
        &self,
        path: &PortableRelativePath,
    ) -> Result<ResolvedPath, FsError> {
        self.verify()?;
        Ok(ResolvedPath::new(self.0.path.join(path.as_str())))
    }

    /// Opens a native path relative to this capability without exposing its physical name.
    pub fn resolve_directory(&self, path: &Path) -> Result<Self, FsError> {
        self.walk_native(path)
    }

    /// Opens a file parent and returns only its direct native component.
    pub fn resolve_file_parent(&self, path: &Path) -> Result<(Self, String), FsError> {
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or(FsError::Path)?;
        single_name(OsStr::new(name))?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        Ok((self.walk_native(parent)?, name.to_owned()))
    }

    /// Derives one portable label from retained containment and a direct child name.
    pub fn descendant_label(
        &self,
        parent: &Self,
        name: &str,
    ) -> Result<PortableRelativePath, FsError> {
        single_name(OsStr::new(name))?;
        if !self.contains_directory(parent)? {
            return Err(FsError::Path);
        }
        let path = parent.0.path.join(name);
        let relative = path.strip_prefix(&self.0.path).map_err(|_| FsError::Path)?;
        PortableRelativePath::new(relative.to_str().ok_or(FsError::Path)?)
    }

    /// Interprets an emitted native path and returns its portable descendant label.
    pub fn emitted_label(&self, path: &Path) -> Result<PortableRelativePath, FsError> {
        let (parent, name) = self.resolve_file_parent(path)?;
        self.descendant_label(&parent, &name)
    }

    /// Configures a child process current directory without exposing a native path.
    pub fn set_current_dir<'a>(
        &self,
        command: &'a mut Command,
    ) -> Result<&'a mut Command, FsError> {
        self.verify()?;
        Ok(command.current_dir(&self.0.path))
    }

    /// Adds this verified directory as one process argument.
    pub fn push_argument<'a>(&self, command: &'a mut Command) -> Result<&'a mut Command, FsError> {
        self.verify()?;
        Ok(command.arg(&self.0.path))
    }

    /// Adds a portable child of this verified directory as one process argument.
    pub fn push_descendant_argument<'a>(
        &self,
        command: &'a mut Command,
        path: &PortableRelativePath,
    ) -> Result<&'a mut Command, FsError> {
        self.verify()?;
        Ok(command.arg(self.0.path.join(path.as_str())))
    }

    /// Renders a relative reference from a portable descendant to another capability.
    pub fn reference_from_descendant(
        &self,
        from: &PortableRelativePath,
        to: &Self,
    ) -> Result<String, FsError> {
        self.verify()?;
        to.verify()?;
        relative_reference(&self.0.path.join(from.as_str()), &to.0.path)
    }

    /// Renders a relative reference from one retained directory to another.
    pub fn reference_to(&self, to: &Self) -> Result<String, FsError> {
        self.verify()?;
        to.verify()?;
        relative_reference(&self.0.path, &to.0.path)
    }

    /// Renders a project-relative reference to a native input resolved by a base capability.
    pub fn reference_from_child_to_path(
        &self,
        child: &OsStr,
        target_base: &Self,
        target: &Path,
    ) -> Result<String, FsError> {
        single_name(child)?;
        self.verify()?;
        let (parent, name) = target_base.resolve_file_parent(target)?;
        let target = parent.0.path.join(name);
        relative_reference(&self.0.path.join(child), &target)
    }
}

fn relative_reference(from: &Path, to: &Path) -> Result<String, FsError> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return Err(FsError::Path);
    }
    let mut parts = vec![".."; from.len() - common];
    for component in &to[common..] {
        let Component::Normal(name) = component else {
            return Err(FsError::Path);
        };
        parts.push(name.to_str().ok_or(FsError::Path)?);
    }
    Ok(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}
