//! Mock observation types over retained host file identities.

use std::sync::Arc;

use chigiri_host::filesystem::FileIdentity;

use super::{Directory, ProjectError, RawSnapshot, Snapshot, common, map};

#[derive(Clone)]
pub(in crate::project) enum Observation {
    Present(Arc<Identity>),
    Absent(Directory, std::ffi::OsString),
}

pub(in crate::project) struct Identity(Arc<chigiri_host::filesystem::ObservedFile>);

impl Directory {
    pub fn observe(&self, path: &str) -> Result<Observation, ProjectError> {
        self.0
            .observe(path)
            .map(|observation| match observation {
                chigiri_host::filesystem::FileObservation::Present(file) => {
                    Observation::Present(Arc::new(Identity(file)))
                }
                chigiri_host::filesystem::FileObservation::Absent(parent, name) => {
                    Observation::Absent(Self(parent), name)
                }
            })
            .map_err(|error| map(error, "path depth limit"))
    }
}

impl Identity {
    pub fn identity(&self) -> FileIdentity {
        self.0.identity()
    }

    pub fn bytes(&self) -> u64 {
        self.0.bytes()
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        self.0.verify().map_err(common)
    }

    pub fn read(&self, maximum: usize) -> Result<RawSnapshot, ProjectError> {
        let snapshot = self
            .0
            .read(maximum)
            .map_err(|error| map(error, "input file byte limit"))?;
        let bytes = snapshot.bytes().to_vec();
        Ok(Snapshot { snapshot, bytes })
    }
}

impl Observation {
    pub fn verify(&self) -> Result<(), ProjectError> {
        match self {
            Self::Present(identity) => identity.verify(),
            Self::Absent(parent, name) => parent.absent(name),
        }
    }

    pub fn exists(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}
