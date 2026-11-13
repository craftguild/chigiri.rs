//! Bounded audit-mode events and a fail-open sink contract.

use std::error::Error;
use std::fmt;

use chigiri_http::BoxFuture;
use chigiri_runtime::{DocumentDigest, RequestIdentity};

use crate::OperationSourcePolicy;

/// Outcome of matching a request against the operation catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationAuditOutcome {
    Hit,
    Miss,
    ProviderUnavailable,
}

/// The audit event of the operation source phase that does not contain raw document.
///
/// During source resolution, [`Self::identity`] has no selected operation because
/// operation selection has not run yet. Trace sampling may be `Drop`, but that
/// decision never suppresses this security audit event.
#[derive(Clone, Copy)]
pub struct OperationAuditEvent<'a> {
    identity: RequestIdentity<'a>,
    document_digest: DocumentDigest,
    catalog_revision: Option<&'a str>,
    mode: OperationSourcePolicy,
    outcome: OperationAuditOutcome,
}

impl<'a> OperationAuditEvent<'a> {
    pub(crate) const fn new(
        identity: RequestIdentity<'a>,
        document_digest: DocumentDigest,
        catalog_revision: Option<&'a str>,
        mode: OperationSourcePolicy,
        outcome: OperationAuditOutcome,
    ) -> Self {
        Self {
            identity,
            document_digest,
            catalog_revision,
            mode,
            outcome,
        }
    }

    /// Returns the correlation identity borrowed from the request envelope.
    ///
    /// A sink that retains the event must copy any bounded identifiers before
    /// `record` completes.
    pub const fn identity(&self) -> RequestIdentity<'a> {
        self.identity
    }
    pub const fn document_digest(&self) -> DocumentDigest {
        self.document_digest
    }
    pub const fn catalog_revision(&self) -> Option<&'a str> {
        self.catalog_revision
    }
    pub const fn mode(&self) -> OperationSourcePolicy {
        self.mode
    }
    pub const fn outcome(&self) -> OperationAuditOutcome {
        self.outcome
    }
}

impl fmt::Debug for OperationAuditEvent<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperationAuditEvent")
            .field("identity", &self.identity)
            .field("document_digest", &self.document_digest)
            .field("has_catalog_revision", &self.catalog_revision.is_some())
            .field("mode", &self.mode)
            .field("outcome", &self.outcome)
            .finish()
    }
}

/// An audit sink error whose source is available only to the local observer.
pub struct OperationAuditSinkError(Box<dyn Error + Send + Sync>);

impl OperationAuditSinkError {
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for OperationAuditSinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperationAuditSinkError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OperationAuditSinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation audit sink failed")
    }
}

impl Error for OperationAuditSinkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Object-safe sink that records audit-mode events asynchronously.
pub trait OperationAuditSink: Send + Sync + 'static {
    /// Records the borrowed event, copying data that must outlive the returned future.
    fn record<'a>(
        &'a self,
        event: OperationAuditEvent<'a>,
    ) -> BoxFuture<'a, Result<(), OperationAuditSinkError>>;
}
