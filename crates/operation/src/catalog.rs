//! In-memory operation catalog and offline listing contract.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use chigiri_http::BoxFuture;
use chigiri_runtime::{DocumentDigest, OperationName};

use crate::digest::document_digest;
use crate::{
    CatalogRevision, OperationRegistry, PersistedOperationId, ProviderError, RegistryDocument,
};

/// Hard limits on catalog entries and document bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogLimits {
    pub max_entries: usize,
    pub max_document_bytes: usize,
    pub max_total_document_bytes: usize,
}

impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_024,
            max_document_bytes: 256 * 1_024,
            max_total_document_bytes: 16 * 1_024 * 1_024,
        }
    }
}

/// An exact persisted-operation entry supplied by the application.
#[derive(Clone, Eq, PartialEq)]
pub struct CatalogEntry {
    id: PersistedOperationId,
    document: Arc<str>,
    operation_name: Option<OperationName>,
    cost_hint: Option<u64>,
}

impl fmt::Debug for CatalogEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogEntry")
            .field("id", &self.id)
            .field("document_bytes", &self.document.len())
            .field("named", &self.operation_name.is_some())
            .field("has_cost_hint", &self.cost_hint.is_some())
            .finish()
    }
}

impl CatalogEntry {
    /// Associates an opaque ID with exact document bytes.
    pub fn new(id: PersistedOperationId, document: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            document: document.into(),
            operation_name: None,
            cost_hint: None,
        }
    }

    /// Associates the selected operation name with a multi-operation document.
    pub fn with_operation_name(mut self, operation_name: OperationName) -> Self {
        self.operation_name = Some(operation_name);
        self
    }

    /// Attaches a validated, nonzero cost hint.
    pub fn with_cost_hint(mut self, cost_hint: u64) -> Result<Self, CatalogMetadataError> {
        if cost_hint == 0 {
            return Err(CatalogMetadataError);
        }
        self.cost_hint = Some(cost_hint);
        Ok(self)
    }

    pub const fn id(&self) -> &PersistedOperationId {
        &self.id
    }
    pub const fn document(&self) -> &Arc<str> {
        &self.document
    }
    pub const fn operation_name(&self) -> Option<&OperationName> {
        self.operation_name.as_ref()
    }
    pub const fn cost_hint(&self) -> Option<u64> {
        self.cost_hint
    }
}

/// The catalog static metadata does not meet the bounded contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogMetadataError;

impl fmt::Display for CatalogMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation catalog metadata is invalid")
    }
}

impl std::error::Error for CatalogMetadataError {}

/// Object-safe contract that lists every entry for offline schema validation.
pub trait OperationCatalog: Send + Sync + 'static {
    /// Returns the immutable revision in the deployment.
    fn revision(&self) -> &CatalogRevision;
    /// Returns all entries in deterministic source order.
    fn entries(&self) -> &[CatalogEntry];
}

/// Bounded, immutable in-memory catalog.
pub struct InMemoryOperationCatalog {
    revision: CatalogRevision,
    entries: Box<[CatalogEntry]>,
    index: BTreeMap<PersistedOperationId, usize>,
    digest_index: BTreeMap<DocumentDigest, usize>,
}

impl InMemoryOperationCatalog {
    /// Validates all limits and ID consistency before building an immutable catalog.
    pub fn new(
        revision: CatalogRevision,
        entries: Vec<CatalogEntry>,
        limits: CatalogLimits,
    ) -> Result<Self, CatalogBuildError> {
        if entries.len() > limits.max_entries {
            return Err(CatalogBuildError::LimitExceeded);
        }
        let mut total = 0usize;
        let mut seen = BTreeSet::new();
        let mut index = BTreeMap::new();
        let mut digest_index: BTreeMap<DocumentDigest, usize> = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            if entry.document.is_empty() || entry.document.len() > limits.max_document_bytes {
                return Err(CatalogBuildError::LimitExceeded);
            }
            total = total
                .checked_add(entry.document.len())
                .ok_or(CatalogBuildError::LimitExceeded)?;
            if total > limits.max_total_document_bytes {
                return Err(CatalogBuildError::LimitExceeded);
            }
            if !seen.insert(entry.id.clone()) {
                return Err(CatalogBuildError::DuplicateId);
            }
            index.insert(entry.id.clone(), position);
            let digest = document_digest(&entry.document);
            if let Some(previous) = digest_index.get(&digest).copied() {
                if entries[previous].cost_hint != entry.cost_hint {
                    return Err(CatalogBuildError::ConflictingDocumentMetadata);
                }
            } else {
                digest_index.insert(digest, position);
            }
        }
        Ok(Self {
            revision,
            entries: entries.into_boxed_slice(),
            index,
            digest_index,
        })
    }
}

impl OperationCatalog for InMemoryOperationCatalog {
    fn revision(&self) -> &CatalogRevision {
        &self.revision
    }
    fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }
}

impl OperationRegistry for InMemoryOperationCatalog {
    fn lookup<'a>(
        &'a self,
        id: &'a PersistedOperationId,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            self.index
                .get(id)
                .map(|index| registry_document(&self.entries[*index]))
                .transpose()
        })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        digest: &'a DocumentDigest,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            self.digest_index
                .get(digest)
                .map(|index| registry_document(&self.entries[*index]))
                .transpose()
        })
    }

    fn revision(&self) -> Option<&str> {
        Some(self.revision.as_str())
    }
}

/// Failure to build a valid bounded catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogBuildError {
    LimitExceeded,
    DuplicateId,
    ConflictingDocumentMetadata,
}

impl fmt::Display for CatalogBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded => {
                formatter.write_str("operation catalog exceeds configured limits")
            }
            Self::DuplicateId => formatter.write_str("operation catalog contains a duplicate ID"),
            Self::ConflictingDocumentMetadata => {
                formatter.write_str("operation aliases contain conflicting document metadata")
            }
        }
    }
}

impl std::error::Error for CatalogBuildError {}

fn registry_document(entry: &CatalogEntry) -> Result<RegistryDocument, ProviderError> {
    let document = RegistryDocument::new(Arc::clone(entry.document()));
    match entry.cost_hint() {
        Some(cost) => document.with_cost_hint(cost).map_err(ProviderError::new),
        None => Ok(document),
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogBuildError, CatalogEntry, CatalogLimits, InMemoryOperationCatalog};
    use crate::{CatalogRevision, PersistedOperationId};

    fn entry(id: &str) -> CatalogEntry {
        CatalogEntry::new(
            PersistedOperationId::new(id).unwrap(),
            "query Catalog { value }",
        )
    }

    #[test]
    fn build_rejects_duplicate_ids_and_entry_limit_overflow() {
        let revision = CatalogRevision::new("revision-1").unwrap();
        assert_eq!(
            InMemoryOperationCatalog::new(
                revision.clone(),
                vec![entry("same"), entry("same")],
                CatalogLimits::default(),
            )
            .err(),
            Some(CatalogBuildError::DuplicateId),
        );
        let limits = CatalogLimits {
            max_entries: 0,
            ..CatalogLimits::default()
        };
        assert_eq!(
            InMemoryOperationCatalog::new(revision, vec![entry("one")], limits).err(),
            Some(CatalogBuildError::LimitExceeded),
        );
    }
}
