//! Track initially absent parent directories separately from parents created later by the transaction.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use super::super::ProjectError;
use super::super::filesystem::Directory;
use super::super::update_inputs::failure;
use chigiri_host::PortableRelativePath;

struct Parent {
    directory: Option<Directory>,
    existed: bool,
}

pub(super) struct Parents {
    paths: BTreeMap<Option<PortableRelativePath>, Parent>,
    uncertain: Vec<PortableRelativePath>,
}

impl Parents {
    pub fn capture<'a>(
        root: &Directory,
        paths: impl Iterator<Item = &'a PortableRelativePath>,
    ) -> Result<Self, ProjectError> {
        let mut result = Self {
            paths: BTreeMap::from([(
                None,
                Parent {
                    directory: Some(root.clone()),
                    existed: true,
                },
            )]),
            uncertain: Vec::new(),
        };
        for path in paths {
            let Some(parent) = path.parent() else {
                continue;
            };
            let mut prefix: Option<PortableRelativePath> = None;
            for part in parent.components() {
                let old = prefix.clone();
                let component = PortableRelativePath::new(part).map_err(|_| invalid())?;
                prefix = Some(match &prefix {
                    Some(prefix) => prefix.join(&component).map_err(|_| invalid())?,
                    None => component,
                });
                if result.paths.contains_key(&prefix) {
                    continue;
                }
                let directory = result.paths[&old]
                    .directory
                    .as_ref()
                    .map(|directory| directory.optional_child(OsStr::new(part)))
                    .transpose()?
                    .flatten();
                let existed = directory.is_some();
                result
                    .paths
                    .insert(prefix.clone(), Parent { directory, existed });
            }
        }
        result.verify_before()?;
        Ok(result)
    }

    pub fn obtain(
        &mut self,
        path: &Option<PortableRelativePath>,
    ) -> Result<Directory, ProjectError> {
        if let Some(directory) = &self.paths.get(path).ok_or_else(invalid)?.directory {
            directory.verify()?;
            return Ok(directory.clone());
        }
        let path = path.as_ref().ok_or_else(invalid)?;
        let parent_path = path.parent();
        let parent = self.obtain(&parent_path)?;
        let directory = parent
            .create_directory_observed(OsStr::new(path.file_name()))
            .map_err(|failure| {
                if failure.created {
                    self.uncertain.push(path.clone());
                }
                let mut error = *failure.error;
                error.set_file(path);
                error
            })?;
        self.paths
            .get_mut(&Some(path.clone()))
            .ok_or_else(invalid)?
            .directory = Some(directory.clone());
        Ok(directory)
    }

    pub fn verify_before(&self) -> Result<(), ProjectError> {
        for (path, parent) in &self.paths {
            if let Some(directory) = &parent.directory {
                directory.verify()?;
            } else if let Some(path) = path
                && let Some(directory) = &self.paths[&path.parent()].directory
            {
                directory.absent(OsStr::new(path.file_name()))?;
            }
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        for parent in self.paths.values() {
            if let Some(directory) = &parent.directory {
                directory.verify()?;
            }
        }
        if !self.uncertain.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn rollback(&self, errors: &mut Vec<ProjectError>) {
        for (path, parent) in self.paths.iter().rev() {
            if !parent.existed
                && let Some(directory) = &parent.directory
                && let Err(mut error) = directory.remove_empty()
            {
                if let Some(path) = path {
                    error.set_file(path.clone());
                }
                errors.push(error);
            }
        }
        for path in &self.uncertain {
            errors.push(failure(
                path,
                "recovery",
                "new directory ownership could not be verified",
            ));
        }
    }
}

fn invalid() -> ProjectError {
    ProjectError::new("ownership", "invalid or changed transaction parent")
}
