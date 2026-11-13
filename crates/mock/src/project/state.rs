//! Bind the pristine base, latest proposal ownership, and active or orphan files to one generation snapshot.

use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

use super::ProjectError;
use super::config_stamp::ConfigStamp;
use super::digest::Digest;
use super::metadata::{self, Entries};
use super::normalize::AdoptedSchema;
use super::summary::Summary;

pub(super) const FORMAT: &str = "chigiri-mock-update-state/v11";
pub(super) const SNAPSHOT_FORMAT: &str = "chigiri-mock-schema-snapshot/v11";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dist {
    pub path: PortableRelativePath,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Resolver {
    pub coordinate: String,
    pub path: PortableRelativePath,
    pub base: Digest,
    pub candidate: Digest,
    pub dist: Option<Dist>,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Seed {
    pub model: String,
    pub path: PortableRelativePath,
    pub active: bool,
}

/// Registered module ownership; inactive entries preserve removed authored files.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Consumer {
    pub name: String,
    pub field: String,
    pub path: PortableRelativePath,
    pub base: Digest,
    pub candidate: Digest,
    pub dist: Option<Dist>,
    pub active: bool,
}

/// Owned fixed-slot hook metadata retained regardless of whether the hook is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Hook {
    pub slot: crate::hooks::HookSlot,
    pub path: PortableRelativePath,
    pub base: Digest,
    pub candidate: Digest,
    pub dist: Option<Dist>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub format: String,
    pub generation: Digest,
    pub public_digest: Digest,
    pub schema_digest: String,
    pub types_digest: Digest,
    pub summary_digest: Digest,
    pub snapshot_digest: Digest,
    pub config: ConfigStamp,
    pub hooks: Entries<Hook>,
    pub resolvers: Entries<Resolver>,
    pub seeds: Entries<Seed>,
    pub consumers: Entries<Consumer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub format: String,
    pub generation: Digest,
    pub public_schema: AdoptedSchema,
    pub schema_digest: String,
    pub summary: Summary,
    pub config: ConfigStamp,
}

impl State {
    pub fn render_documents(
        &mut self,
        snapshot: &mut Snapshot,
        types: &str,
    ) -> Result<(String, String), ProjectError> {
        if self.file_count() > metadata::MAX_FILES {
            return Err(ProjectError::new("limit", "managed file count limit"));
        }
        let remaining = metadata::MAX_BYTES
            .checked_sub(types.len())
            .ok_or_else(|| ProjectError::new("limit", "generated metadata byte limit"))?;
        self.types_digest = Digest::bytes(types.as_bytes());
        self.summary_digest =
            Digest::bytes(metadata::render(&snapshot.summary, remaining)?.as_bytes());
        self.public_digest = snapshot.public_schema.digest.clone();
        self.generation = self.identity()?;
        snapshot.generation = self.generation.clone();
        snapshot.schema_digest.clone_from(&self.schema_digest);
        let snapshot_bytes = metadata::render(snapshot, remaining)?;
        self.snapshot_digest = Digest::bytes(snapshot_bytes.as_bytes());
        self.verify(types, snapshot, &snapshot_bytes)?;
        let state_bytes = metadata::render(self, remaining - snapshot_bytes.len())?;
        Ok((snapshot_bytes, state_bytes))
    }

    pub fn verify(
        &self,
        types: &str,
        snapshot: &Snapshot,
        snapshot_bytes: &str,
    ) -> Result<(), ProjectError> {
        if self.format != FORMAT || snapshot.format != SNAPSHOT_FORMAT {
            return Err(ProjectError::new(
                "ownership",
                "unsupported generated metadata format",
            ));
        }
        snapshot.public_schema.verify()?;
        let summary = metadata::render(&snapshot.summary, metadata::MAX_BYTES)?;
        if self.config != snapshot.config
            || self.file_count() > metadata::MAX_FILES
            || self.resolvers.0.iter().filter(|entry| entry.active).count() > crate::limits::SCRIPTS
            || self.seeds.0.iter().filter(|entry| entry.active).count() > crate::limits::MODELS
            || self.consumers.0.iter().filter(|entry| entry.active).count()
                > crate::events::EventSettings::MAX_CONSUMERS
            || !metadata::name(&snapshot.summary.query)
            || snapshot
                .summary
                .mutation
                .as_ref()
                .is_some_and(|name| !metadata::name(name))
            || chigiri::core::SchemaDigest::from_hex(&self.schema_digest).is_err()
            || self.schema_digest != snapshot.schema_digest
            || self.public_digest != snapshot.public_schema.digest
            || self.generation != snapshot.generation
            || self.generation != self.identity()?
            || !self.types_digest.matches(types.as_bytes())
            || !self.summary_digest.matches(summary.as_bytes())
            || !self.snapshot_digest.matches(snapshot_bytes.as_bytes())
        {
            return Err(ProjectError::new(
                "ownership",
                "invalid generated ownership or generation",
            ));
        }
        super::inventory::validate(self, snapshot)
    }

    pub fn identity(&self) -> Result<Digest, ProjectError> {
        #[derive(Serialize)]
        struct Content<'a> {
            format: &'a str,
            schema_digest: &'a str,
            public_digest: &'a Digest,
            types_digest: &'a Digest,
            summary_digest: &'a Digest,
            config: &'a ConfigStamp,
            hooks: &'a Entries<Hook>,
            resolvers: &'a Entries<Resolver>,
            seeds: &'a Entries<Seed>,
            consumers: &'a Entries<Consumer>,
        }
        let content = Content {
            format: &self.format,
            schema_digest: &self.schema_digest,
            public_digest: &self.public_digest,
            types_digest: &self.types_digest,
            summary_digest: &self.summary_digest,
            config: &self.config,
            hooks: &self.hooks,
            resolvers: &self.resolvers,
            seeds: &self.seeds,
            consumers: &self.consumers,
        };
        Ok(Digest::bytes(
            metadata::render(&content, metadata::MAX_BYTES)?.as_bytes(),
        ))
    }

    pub fn file_count(&self) -> usize {
        // Count configuration, generated types, schema snapshot, and update-state metadata before managed entries.
        4 + self.resolvers.0.len()
            + self.seeds.0.len()
            + self.hooks.0.len()
            + self.consumers.0.len()
            + self
                .consumers
                .0
                .iter()
                .filter(|entry| entry.dist.is_some())
                .count()
            + self
                .hooks
                .0
                .iter()
                .filter(|entry| entry.dist.is_some())
                .count()
            + self
                .resolvers
                .0
                .iter()
                .filter(|entry| entry.dist.is_some())
                .count()
    }
}
