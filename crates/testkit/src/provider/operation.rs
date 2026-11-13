//! Shared conformance contract for persisted-operation providers.

use chigiri::DocumentDigest;
use chigiri_http::BoxFuture;
use chigiri_operation::{OperationRegistry, PersistedOperationId, ProviderError, RegistryDocument};

use super::ProviderConformanceError;

/// Wrapper that preserves operation-registry object safety and borrow lifetimes.
pub struct OperationProviderConformance<'a> {
    provider: &'a dyn OperationRegistry,
}

impl<'a> OperationProviderConformance<'a> {
    pub const fn new(provider: &'a dyn OperationRegistry) -> Self {
        Self { provider }
    }

    pub fn lookup<'b>(
        &'b self,
        id: &'b PersistedOperationId,
    ) -> BoxFuture<'b, Result<Option<RegistryDocument>, ProviderError>> {
        self.provider.lookup(id)
    }

    pub fn lookup_by_document_digest<'b>(
        &'b self,
        digest: &'b DocumentDigest,
    ) -> BoxFuture<'b, Result<Option<RegistryDocument>, ProviderError>> {
        self.provider.lookup_by_document_digest(digest)
    }
}

/// Known/missing ID and digest fixture for the operation provider.
pub struct OperationProviderCase<'a> {
    pub known_id: &'a PersistedOperationId,
    pub known_digest: &'a DocumentDigest,
    pub expected_document: &'a str,
    pub missing_id: &'a PersistedOperationId,
    pub missing_digest: &'a DocumentDigest,
}

/// Verifies the ID, digest, and revision contract of a third-party operation provider.
pub async fn assert_operation_provider_conformance(
    provider: &dyn OperationRegistry,
    case: OperationProviderCase<'_>,
) -> Result<(), ProviderConformanceError> {
    let known = lookup_id(provider, case.known_id, "known lookup failed").await?;
    assert_document(
        known,
        case.expected_document,
        "known lookup returned missing",
    )?;
    if lookup_id(provider, case.missing_id, "missing lookup failed")
        .await?
        .is_some()
    {
        return Err(ProviderConformanceError::new(
            "missing lookup returned a document",
        ));
    }
    let digest = provider
        .lookup_by_document_digest(case.known_digest)
        .await
        .map_err(|_| ProviderConformanceError::new("known digest lookup failed"))?;
    assert_document(
        digest,
        case.expected_document,
        "known digest lookup returned missing",
    )?;
    if provider
        .lookup_by_document_digest(case.missing_digest)
        .await
        .map_err(|_| ProviderConformanceError::new("missing digest lookup failed"))?
        .is_some()
    {
        return Err(ProviderConformanceError::new(
            "missing digest lookup returned a document",
        ));
    }
    if provider.revision().is_some_and(|revision| {
        revision.is_empty()
            || revision.len() > 128
            || !revision.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
    }) {
        return Err(ProviderConformanceError::new(
            "provider revision is outside the bounded contract",
        ));
    }
    Ok(())
}

async fn lookup_id(
    provider: &dyn OperationRegistry,
    id: &PersistedOperationId,
    message: &'static str,
) -> Result<Option<RegistryDocument>, ProviderConformanceError> {
    provider
        .lookup(id)
        .await
        .map_err(|_| ProviderConformanceError::new(message))
}

fn assert_document(
    document: Option<RegistryDocument>,
    expected: &str,
    missing: &'static str,
) -> Result<(), ProviderConformanceError> {
    let document = document.ok_or_else(|| ProviderConformanceError::new(missing))?;
    if document.document() != expected {
        return Err(ProviderConformanceError::new(
            "provider returned a different document",
        ));
    }
    Ok(())
}
