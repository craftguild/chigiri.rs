#![forbid(unsafe_code)]
//! Bounded server-side provider contract for persisted operations.

mod audit;
mod catalog;
mod catalog_validation;
mod digest;
mod id;
mod provider;
mod source;

pub use audit::{
    OperationAuditEvent, OperationAuditOutcome, OperationAuditSink, OperationAuditSinkError,
};
pub use catalog::{
    CatalogBuildError, CatalogEntry, CatalogLimits, CatalogMetadataError, InMemoryOperationCatalog,
    OperationCatalog,
};
pub use catalog_validation::{
    CatalogValidationError, ValidatedCatalog, ValidatedCatalogEntry, validate_catalog,
};
pub use id::{CatalogRevision, PersistedOperationId, PersistedOperationIdError};
pub use provider::{
    ApqStore, InMemoryApqStore, OperationRegistry, OperationSourcePolicy, ProviderError,
    RegistryDocument, RegistryMetadataError, StoreLimits,
};
pub use source::PersistedOperationResolver;
