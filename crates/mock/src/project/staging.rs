//! Track a private new tree and clean up only verified files and empty directories on failure.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::filesystem::{Directory, FileSnapshot};
use super::{ProjectError, metadata};
use chigiri_host::PortableRelativePath;

pub(super) struct Staging {
    root: Directory,
    directories: BTreeMap<Option<PortableRelativePath>, Directory>,
    files: BTreeMap<PortableRelativePath, FileSnapshot>,
}

impl Staging {
    pub fn new(parent: &Directory) -> Result<Self, ProjectError> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        for _ in 0..32 {
            let name = format!(
                ".chigiri-mock-init-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            match parent.create_directory(OsStr::new(&name)) {
                Ok(root) => {
                    return Ok(Self {
                        directories: BTreeMap::from([(None, root.clone())]),
                        root,
                        files: BTreeMap::new(),
                    });
                }
                Err(error) if error.message == "exclusive destination already exists" => {}
                Err(error) => return Err(error),
            }
        }
        Err(ProjectError::new(
            "ownership",
            "cannot reserve private staging directory",
        ))
    }

    pub fn write(
        &mut self,
        files: &BTreeMap<PortableRelativePath, String>,
    ) -> Result<(), ProjectError> {
        if files.len() > metadata::MAX_FILES {
            return Err(ProjectError::new("limit", "managed file count limit"));
        }
        for (path, bytes) in files {
            let mut current = self.root.clone();
            let mut prefix: Option<PortableRelativePath> = None;
            let mut parts = Path::new(path.as_str()).components().peekable();
            while let Some(part) = parts.next() {
                let Component::Normal(name) = part else {
                    return Err(ProjectError::new("ownership", "invalid staging path"));
                };
                if parts.peek().is_none() {
                    self.files
                        .insert(path.clone(), current.create_file(name, bytes)?);
                } else {
                    let component = PortableRelativePath::new(name.to_str().ok_or_else(invalid)?)
                        .map_err(|_| invalid())?;
                    prefix = Some(match &prefix {
                        Some(prefix) => prefix.join(&component).map_err(|_| invalid())?,
                        None => component,
                    });
                    current = match self.directories.get(&prefix) {
                        Some(directory) => directory.clone(),
                        None => {
                            let directory = current.create_directory(name)?;
                            self.directories.insert(prefix.clone(), directory.clone());
                            directory
                        }
                    };
                }
            }
        }
        self.verify_at(&self.root)
    }

    fn verify_at(&self, root: &Directory) -> Result<(), ProjectError> {
        for (path, original) in &self.directories {
            let directory = if path.is_none() {
                root.clone()
            } else {
                let mut directory = root.clone();
                for part in Path::new(path.as_ref().unwrap().as_str()).components() {
                    directory = directory.child(part.as_os_str())?;
                }
                directory
            };
            if directory.identity() != original.identity() {
                return Err(invalid());
            }
            let expected: BTreeSet<_> = self
                .directories
                .keys()
                .filter_map(|path| path.as_ref().map(PortableRelativePath::as_str))
                .chain(self.files.keys().map(|path| path.as_str()))
                .filter(|child| {
                    Path::new(child).parent()
                        == Some(Path::new(
                            path.as_ref().map_or("", PortableRelativePath::as_str),
                        ))
                })
                .filter_map(|child| Path::new(child).file_name().map(ToOwned::to_owned))
                .collect();
            let actual: BTreeSet<_> = directory
                .entries(metadata::MAX_FILES * 2)?
                .into_iter()
                .collect();
            if actual != expected {
                return Err(invalid());
            }
        }
        for (path, file) in &self.files {
            file.verify_at(root, path)?;
        }
        Ok(())
    }

    pub fn publish(&self, parent: &Directory, name: &OsStr) -> Result<(), ProjectError> {
        self.verify_at(&self.root)?;
        let root = self.root.move_to(parent, name).map_err(|failure| {
            let mut error = *failure.error;
            error.committed = failure.moved.is_some();
            error
        })?;
        self.verify_at(&root).map_err(|mut error| {
            error.committed = true;
            error
        })
    }

    pub fn cleanup(&self, primary: &mut ProjectError) {
        // Every known object is verified. Unknown or modified files and nonempty directories are left untouched.
        for (path, file) in &self.files {
            if let Err(mut error) = file.remove() {
                error.set_file(path.clone());
                primary.recovery.push(error);
            }
        }
        for (path, directory) in self.directories.iter().rev() {
            if let Err(mut error) = directory.remove_empty() {
                error.set_file(path.clone().unwrap_or_else(|| {
                    PortableRelativePath::new("staging").expect("fixed portable path")
                }));
                primary.recovery.push(error);
            }
        }
    }
}

fn invalid() -> ProjectError {
    ProjectError::new("ownership", "private staging tree changed")
}
