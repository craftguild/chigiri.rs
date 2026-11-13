//! Read-only metadata for an immutable compiled schema.

use std::sync::Arc;

use chigiri_core::SchemaDigest;

/// Validated schema input kind; owned schemas do not claim generated-ABI provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaSourceKind {
    /// Uses a generated descriptor and ABI-compatible resolver dispatch.
    Generated,
    /// The dispatch is tied to the owned SDL and instance for the experiment.
    Owned,
}

/// Read-only information about a validated schema and optional catalog revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaInfo {
    framework_version: &'static str,
    generated_abi: Option<u32>,
    source_kind: SchemaSourceKind,
    schema_digest: SchemaDigest,
    catalog_revision: Option<Arc<str>>,
}

impl SchemaInfo {
    /// Creates immutable metadata from a generated wrapper.
    #[doc(hidden)]
    pub const fn generated(
        framework_version: &'static str,
        generated_abi: u32,
        schema_digest: SchemaDigest,
    ) -> Self {
        Self {
            framework_version,
            generated_abi: Some(generated_abi),
            source_kind: SchemaSourceKind::Generated,
            schema_digest,
            catalog_revision: None,
        }
    }

    pub(crate) const fn owned(
        framework_version: &'static str,
        schema_digest: SchemaDigest,
    ) -> Self {
        Self {
            framework_version,
            generated_abi: None,
            source_kind: SchemaSourceKind::Owned,
            schema_digest,
            catalog_revision: None,
        }
    }

    /// Generated descriptor and owned SDL are clearly distinguished.
    pub const fn source_kind(&self) -> SchemaSourceKind {
        self.source_kind
    }

    /// Returns the framework crate version.
    pub const fn framework_version(&self) -> &'static str {
        self.framework_version
    }

    /// Returns the ABI version for generated input, or `None` for owned input.
    pub const fn generated_abi(&self) -> Option<u32> {
        self.generated_abi
    }

    /// Returns the schema digest derived from the input sources.
    pub const fn schema_digest(&self) -> SchemaDigest {
        self.schema_digest
    }

    /// Returns the catalog revision that the production preset has verified.
    pub fn catalog_revision(&self) -> Option<&str> {
        self.catalog_revision.as_deref()
    }

    /// Associates a validated catalog revision with the immutable view.
    #[doc(hidden)]
    pub fn with_catalog_revision(mut self, revision: Arc<str>) -> Self {
        self.catalog_revision = Some(revision);
        self
    }

    pub(crate) fn with_optional_catalog_revision(mut self, revision: Option<Arc<str>>) -> Self {
        self.catalog_revision = revision;
        self
    }
}
