//! Track the three CLI-owned generated artifacts as one generation with their installed bytes.

use std::path::Path;

use super::filesystem::{Directory, FileSnapshot};
use super::metadata;
use super::state::{Snapshot, State};
use super::{Layout, ProjectError};
use crate::ir::MockIr;

pub(super) struct Management {
    pub state: State,
    pub snapshot: Snapshot,
    pub files: Vec<FileSnapshot>,
}

impl Management {
    pub fn read(directory: &Directory) -> Result<Self, ProjectError> {
        let mut files = Vec::new();
        let mut remaining = metadata::MAX_BYTES;
        for path in [
            "generated/types.d.luau",
            "generated/schema.snapshot.json",
            "generated/update-state.json",
        ] {
            let file = read_file(directory, path, remaining)?;
            remaining -= file.bytes.len();
            files.push(file);
        }
        let snapshot: Snapshot = serde_json::from_str(&files[1].bytes)
            .map_err(|_| invalid("invalid schema snapshot"))?;
        let state: State =
            serde_json::from_str(&files[2].bytes).map_err(|_| invalid("invalid update state"))?;
        state.verify(&files[0].bytes, &snapshot, &files[1].bytes)?;
        Ok(Self {
            state,
            snapshot,
            files,
        })
    }

    pub fn current(
        &self,
        ir: &MockIr,
        layout: &Layout,
        settings: &super::ProjectSettings,
    ) -> Result<(), ProjectError> {
        if self.state.schema_digest != ir.graph().source().digest().to_hex()
            || self.snapshot.summary != super::summary::build(ir)?
        {
            return Err(invalid("local SDL changed; run chigiri mock codegen"));
        }
        if self.state.config
            != super::config_stamp::ConfigStamp::new(
                settings.hooks,
                &settings.state,
                &settings.clock,
                &settings.events,
            )?
        {
            return Err(invalid(
                "configuration contract changed; run chigiri mock codegen",
            ));
        }
        super::consumer_inventory::current(&self.state, ir, layout)?;
        let resolvers: Vec<_> = self.state.resolvers.0.iter().filter(|e| e.active).collect();
        let seeds: Vec<_> = self.state.seeds.0.iter().filter(|e| e.active).collect();
        if resolvers.len() != layout.resolvers.len()
            || seeds.len() != layout.seeds.len()
            || layout.resolvers.iter().any(|entry| {
                !resolvers
                    .iter()
                    .any(|old| old.coordinate == entry.coordinate && old.path == entry.path)
            })
            || layout.seeds.iter().any(|entry| {
                let model = &ir.graph().types()[ir.models()[entry.model.0].ty.0].name;
                !seeds
                    .iter()
                    .any(|old| old.model == model.as_ref() && old.path == entry.path)
            })
        {
            return Err(invalid("active ownership coverage mismatch"));
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), ProjectError> {
        for file in &self.files {
            file.verify()?;
        }
        Ok(())
    }
}

pub(super) fn read_file(
    directory: &Directory,
    path: &str,
    limit: usize,
) -> Result<FileSnapshot, ProjectError> {
    directory.read(Path::new(path), limit).map_err(|mut error| {
        error.set_file(path);
        error
    })
}

fn invalid(message: &'static str) -> ProjectError {
    let mut error = ProjectError::new("ownership", message);
    error.set_file("generated/update-state.json");
    error
}
