//! Retain update-start snapshots for comparison with prior user bytes and separate seed/orphan identity checks.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::filesystem::{Directory, Observation, RawSnapshot};
use super::{ProjectError, metadata};
use chigiri_host::PortableRelativePath;

pub(super) struct Inputs {
    root: Directory,
    observed: BTreeMap<PortableRelativePath, Observation>,
    bytes: BTreeMap<PortableRelativePath, Arc<RawSnapshot>>,
}

impl Inputs {
    pub fn new(root: Directory) -> Self {
        Self {
            root,
            observed: BTreeMap::new(),
            bytes: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, path: &str, file: RawSnapshot) {
        self.bytes.insert(super::fixed_path(path), Arc::new(file));
    }

    pub fn observe(&mut self, path: &str) -> Result<&Observation, ProjectError> {
        let logical = super::logical(path)?;
        if !self.observed.contains_key(logical.as_str()) {
            // Bound the union of old and new observations used for collision checks.
            if self.observed.len() == metadata::MAX_FILES * 2 {
                return Err(failure(path, "limit", "update observation count limit"));
            }
            let observed = self.root.observe(path).map_err(|mut error| {
                error.set_file(path);
                error
            })?;
            self.observed.insert(logical.clone(), observed);
        }
        Ok(&self.observed[logical.as_str()])
    }

    pub fn size(&mut self, path: &str) -> Result<Option<u64>, ProjectError> {
        match self.observe(path)? {
            Observation::Present(file) => Ok(Some(file.bytes())),
            Observation::Absent(..) => Ok(None),
        }
    }

    pub fn read(
        &mut self,
        path: &str,
        maximum: usize,
    ) -> Result<Option<Arc<RawSnapshot>>, ProjectError> {
        if let Some(file) = self.bytes.get(path) {
            if file.bytes.len() > maximum {
                return Err(failure(path, "limit", "input file byte limit"));
            }
            return Ok(Some(file.clone()));
        }
        let observed = self.observe(path)?.clone();
        let Observation::Present(identity) = observed else {
            return Ok(None);
        };
        let file = Arc::new(identity.read(maximum).map_err(|mut error| {
            error.set_file(path);
            error
        })?);
        self.bytes.insert(super::logical(path)?, file.clone());
        Ok(Some(file))
    }

    pub fn required(
        &mut self,
        path: &str,
        maximum: usize,
    ) -> Result<Arc<RawSnapshot>, ProjectError> {
        self.read(path, maximum)?.ok_or_else(|| missing(path))
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        self.root.verify()?;
        for (path, observed) in &self.observed {
            observed.verify().map_err(|mut error| {
                error.set_file(path.clone());
                error
            })?;
        }
        for (path, file) in &self.bytes {
            file.verify().map_err(|mut error| {
                error.set_file(path.clone());
                error
            })?;
        }
        Ok(())
    }

    pub fn reject_shared_targets(
        &self,
        changed: &BTreeSet<PortableRelativePath>,
    ) -> Result<(), ProjectError> {
        let mut identities = BTreeMap::new();
        for (path, observed) in &self.observed {
            if let Observation::Present(file) = observed {
                identities.insert(path, file.identity());
            }
        }
        for (path, file) in &self.bytes {
            identities.insert(path, file.identity());
        }
        for path in changed {
            if let Some(identity) = identities.get(path)
                && identities
                    .iter()
                    .any(|(other, found)| *other != path && found == identity)
            {
                return Err(failure(
                    path,
                    "ownership",
                    "replacement target shares another input identity",
                ));
            }
        }
        Ok(())
    }

    pub fn verify_untouched(
        &self,
        changed: &BTreeSet<PortableRelativePath>,
    ) -> Result<(), ProjectError> {
        self.root.verify()?;
        for (path, observed) in &self.observed {
            if changed.contains(path) {
                continue;
            }
            match observed {
                Observation::Present(_) => observed.verify()?,
                // Managed orphan targets must remain absent even if their parent directory is newly created.
                Observation::Absent(..) if self.root.observe(path.as_str())?.exists() => {
                    return Err(missing(path.as_str()));
                }
                Observation::Absent(..) => {}
            }
        }
        for (path, file) in &self.bytes {
            if !changed.contains(path) {
                file.verify()?;
            }
        }
        Ok(())
    }
}

pub(super) fn missing(path: &str) -> ProjectError {
    failure(path, "ownership", "active managed input missing or changed")
}

pub(super) fn failure(path: &str, phase: &'static str, message: &'static str) -> ProjectError {
    let mut error = ProjectError::new(phase, message);
    error.set_file(path);
    error
}
