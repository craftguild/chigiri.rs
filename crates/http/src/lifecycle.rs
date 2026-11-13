//! Transport lifecycle contracts independent of any server framework.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use chigiri_core::{GraphQlError, GraphQlRequest};

use crate::{RequestEnvelope, RequestMethod, ResponseMediaType, TransportRequest};

/// Object-safe `Send` future that preserves a borrow lifetime.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Nonnegative delay shared by HTTP `Retry-After` and GraphQL metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryAfter(u64);

impl RetryAfter {
    pub const fn seconds(seconds: u64) -> Self {
        Self(seconds)
    }
    pub const fn as_seconds(self) -> u64 {
        self.0
    }
}

/// Client-safe GraphQL rejection with a validated HTTP status.
#[derive(Clone)]
pub struct Rejection {
    status: u16,
    error: GraphQlError,
    retry_after: Option<RetryAfter>,
}

impl Rejection {
    /// Creates a typed rejection only for status codes from 400 through 599.
    pub fn new(status: u16, error: GraphQlError) -> Result<Self, RejectionError> {
        if !(400..=599).contains(&status) {
            return Err(RejectionError);
        }
        Ok(Self {
            status,
            error,
            retry_after: None,
        })
    }

    /// Sets the retry delay shared by HTTP and GraphQL metadata.
    pub const fn with_retry_after(mut self, retry_after: RetryAfter) -> Self {
        self.retry_after = Some(retry_after);
        self
    }

    pub const fn status(&self) -> u16 {
        self.status
    }
    pub const fn error(&self) -> &GraphQlError {
        &self.error
    }
    pub const fn retry_after(&self) -> Option<RetryAfter> {
        self.retry_after
    }

    /// Encodes the rejection as a GraphQL request error using the negotiated media type.
    pub fn response_parts(&self, media: ResponseMediaType) -> crate::ResponseParts {
        let response = chigiri_core::GraphQlResponse::request_error(self.error.clone());
        let mut parts = crate::encode_response_with_status(media, self.status, &response)
            .unwrap_or_else(|_| crate::ResponseParts::serialization_failure(media));
        if let Some(retry_after) = self.retry_after {
            parts.push_header("retry-after", &retry_after.as_seconds().to_string());
        }
        parts
    }
}

impl fmt::Debug for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Rejection")
            .field("status", &self.status)
            .field("has_retry_after", &self.retry_after.is_some())
            .finish()
    }
}

/// A typed rejection used an invalid HTTP status range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectionError;

impl fmt::Display for RejectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GraphQL rejection status must be between 400 and 599")
    }
}

impl Error for RejectionError {}

/// Interceptor or provider error whose source is not exposed to the client.
pub struct AdmissionError(Box<dyn Error + Send + Sync>);

impl AdmissionError {
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmissionError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GraphQL admission failed")
    }
}

impl Error for AdmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Decision to continue or short-circuit the pipeline.
#[derive(Debug)]
pub enum AdmissionDecision {
    Continue,
    Reject(Rejection),
}

/// Transport-admission contract that runs before body polling.
pub trait TransportAdmission: Send + Sync + 'static {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>>;
}

/// Read-only GraphQL request and transport-policy view after source resolution.
#[derive(Clone, Copy)]
pub struct DecodedRequestView<'a> {
    request: &'a GraphQlRequest,
    method: RequestMethod,
    media: ResponseMediaType,
}

impl<'a> DecodedRequestView<'a> {
    pub const fn new(
        request: &'a GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
    ) -> Self {
        Self {
            request,
            method,
            media,
        }
    }

    pub const fn request(&self) -> &'a GraphQlRequest {
        self.request
    }
    pub const fn method(&self) -> RequestMethod {
        self.method
    }
    pub const fn response_media_type(&self) -> ResponseMediaType {
        self.media
    }
}

impl fmt::Debug for DecodedRequestView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedRequestView")
            .field("method", &self.method)
            .field("media", &self.media)
            .finish_non_exhaustive()
    }
}

/// Typed interceptor that runs after source resolution and before context construction.
pub trait DecodedRequestHook: Send + Sync + 'static {
    fn inspect<'a>(
        &'a self,
        request: DecodedRequestView<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>>;
}

/// Typed failure from an operation-source provider.
#[derive(Debug)]
pub enum SourceResolutionError {
    Reject(Box<Rejection>),
    /// Protocol-level request error encoded with a successful HTTP status.
    Protocol(Box<GraphQlError>),
    /// Provider unavailable error encoded as HTTP 503 while retaining its source.
    Unavailable(AdmissionError),
    Internal(AdmissionError),
}

/// Resolved document and validated catalog metadata passed to the core request.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedOperationSource {
    document: String,
    catalog_cost_hint: Option<u64>,
}

impl fmt::Debug for ResolvedOperationSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedOperationSource")
            .field("document_bytes", &self.document.len())
            .field("has_catalog_cost_hint", &self.catalog_cost_hint.is_some())
            .finish()
    }
}

impl ResolvedOperationSource {
    pub fn new(document: impl Into<String>) -> Self {
        Self {
            document: document.into(),
            catalog_cost_hint: None,
        }
    }

    /// Attaches a nonzero catalog cost hint.
    pub fn with_catalog_cost_hint(mut self, cost_hint: u64) -> Self {
        self.catalog_cost_hint = (cost_hint > 0).then_some(cost_hint);
        self
    }

    pub fn into_document(self) -> String {
        self.document
    }
    pub const fn catalog_cost_hint(&self) -> Option<u64> {
        self.catalog_cost_hint
    }
}

/// Object-safe SPI that resolves a decoded envelope to an exact request document.
pub trait OperationSourceResolver: Send + Sync + 'static {
    fn resolve<'a>(
        &'a self,
        envelope: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>>;
}

#[cfg(test)]
mod tests {
    use super::Rejection;
    use crate::{
        ApplicationMetadata, MetadataField, RequestHeader, ResponseMediaType, TransportRequest,
    };
    use chigiri_core::GraphQlError;

    #[test]
    fn transport_debug_redacts_header_query_and_metadata_values() {
        let metadata = ApplicationMetadata::new(vec![
            MetadataField::new("subject", b"secret-subject".to_vec()).unwrap(),
        ])
        .unwrap();
        let headers = [RequestHeader::new("authorization", b"secret-token")];
        let request = TransportRequest::new(
            "POST",
            "/graphql",
            Some("query=secret"),
            &headers,
            None,
            &metadata,
        )
        .__with_response_media(ResponseMediaType::GraphQlResponseJson);
        let debug = format!("{request:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn rejection_only_receives_400_to_599() {
        assert!(Rejection::new(429, GraphQlError::new("limited")).is_ok());
        assert!(Rejection::new(200, GraphQlError::new("invalid")).is_err());
    }
}
