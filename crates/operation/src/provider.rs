//! Object-safe asynchronous contracts for runtime registry and APQ providers.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use chigiri_http::BoxFuture;

use chigiri_runtime::DocumentDigest;

use crate::PersistedOperationId;

/// Security and transport policy for persisted operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OperationSourcePolicy {
    #[default]
    Disabled,
    Apq,
    Audit,
    Safelist,
    IdOnly,
}

/// An exact immutable document returned by a registry.
#[derive(Clone, Eq, PartialEq)]
pub struct RegistryDocument {
    document: Arc<str>,
    cost_hint: Option<u64>,
}

impl RegistryDocument {
    pub fn new(document: Arc<str>) -> Self {
        Self {
            document,
            cost_hint: None,
        }
    }
    /// Attaches a validated, nonzero catalog cost hint.
    pub fn with_cost_hint(mut self, cost_hint: u64) -> Result<Self, RegistryMetadataError> {
        if cost_hint == 0 {
            return Err(RegistryMetadataError);
        }
        self.cost_hint = Some(cost_hint);
        Ok(self)
    }
    pub fn document(&self) -> &str {
        &self.document
    }
    pub fn into_document(self) -> Arc<str> {
        self.document
    }
    pub const fn cost_hint(&self) -> Option<u64> {
        self.cost_hint
    }
}

impl fmt::Debug for RegistryDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistryDocument")
            .field("document_bytes", &self.document.len())
            .field("has_cost_hint", &self.cost_hint.is_some())
            .finish()
    }
}

/// Registry metadata violates its bounded contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryMetadataError;

impl fmt::Display for RegistryMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation registry metadata is invalid")
    }
}

impl Error for RegistryMetadataError {}

/// Object-safe provider SPI for runtime persisted-operation lookup.
pub trait OperationRegistry: Send + Sync + 'static {
    fn lookup<'a>(
        &'a self,
        id: &'a PersistedOperationId,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>>;

    /// Looks up a catalog entry by the exact document digest.
    fn lookup_by_document_digest<'a>(
        &'a self,
        digest: &'a DocumentDigest,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>>;

    /// Returns the immutable revision exposed through readiness metadata.
    fn revision(&self) -> Option<&str> {
        None
    }
}

/// Bounded APQ lookup and insertion provider SPI.
pub trait ApqStore: OperationRegistry {
    fn put(
        &self,
        id: PersistedOperationId,
        document: Arc<str>,
    ) -> BoxFuture<'_, Result<(), ProviderError>>;
}

/// The provider's default source is not leaked to the client response.
pub struct ProviderError(Box<dyn Error + Send + Sync>);

impl ProviderError {
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation provider failed")
    }
}

impl Error for ProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Entry and document byte limits for the in-memory APQ store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreLimits {
    pub max_entries: usize,
    pub max_document_bytes: usize,
    pub max_total_document_bytes: usize,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_024,
            max_document_bytes: 256 * 1_024,
            max_total_document_bytes: 16 * 1_024 * 1_024,
        }
    }
}

struct StoreState {
    documents: BTreeMap<PersistedOperationId, Arc<str>>,
    order: VecDeque<PersistedOperationId>,
    total_bytes: usize,
}

/// A bounded FIFO APQ store shared only within the process.
pub struct InMemoryApqStore {
    limits: StoreLimits,
    state: Mutex<StoreState>,
}

impl InMemoryApqStore {
    /// Creates an empty store with explicit limits.
    pub fn new(limits: StoreLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(StoreState {
                documents: BTreeMap::new(),
                order: VecDeque::new(),
                total_bytes: 0,
            }),
        }
    }

    fn insert(&self, id: PersistedOperationId, document: Arc<str>) -> Result<(), ProviderError> {
        if self.limits.max_entries == 0
            || document.is_empty()
            || document.len() > self.limits.max_document_bytes
            || document.len() > self.limits.max_total_document_bytes
        {
            return Err(ProviderError::new(StoreLimitError));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ProviderError::new(StorePoisoned))?;
        if let Some(previous) = state.documents.remove(&id) {
            state.total_bytes -= previous.len();
            state.order.retain(|stored| stored != &id);
        }
        while state.documents.len() >= self.limits.max_entries
            || state.total_bytes + document.len() > self.limits.max_total_document_bytes
        {
            let Some(oldest) = state.order.pop_front() else {
                return Err(ProviderError::new(StoreLimitError));
            };
            if let Some(removed) = state.documents.remove(&oldest) {
                state.total_bytes -= removed.len();
            }
        }
        state.total_bytes += document.len();
        state.order.push_back(id.clone());
        state.documents.insert(id, document);
        Ok(())
    }
}

impl OperationRegistry for InMemoryApqStore {
    fn lookup<'a>(
        &'a self,
        id: &'a PersistedOperationId,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .map_err(|_| ProviderError::new(StorePoisoned))?;
            Ok(state.documents.get(id).cloned().map(RegistryDocument::new))
        })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        digest: &'a DocumentDigest,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            let id = PersistedOperationId::new(digest.to_hex())
                .map_err(|_| ProviderError::new(InvalidDigestIdentity))?;
            self.lookup(&id).await
        })
    }
}

impl ApqStore for InMemoryApqStore {
    fn put(
        &self,
        id: PersistedOperationId,
        document: Arc<str>,
    ) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.insert(id, document) })
    }
}

#[derive(Debug)]
struct StoreLimitError;

impl fmt::Display for StoreLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("APQ store limit exceeded")
    }
}

impl Error for StoreLimitError {}

#[derive(Debug)]
struct StorePoisoned;

impl fmt::Display for StorePoisoned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("APQ store lock is poisoned")
    }
}

impl Error for StorePoisoned {}

#[derive(Debug)]
struct InvalidDigestIdentity;

impl fmt::Display for InvalidDigestIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("document digest could not be converted to an APQ identity")
    }
}

impl Error for InvalidDigestIdentity {}
