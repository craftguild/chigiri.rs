//! HTTP lifecycle interceptor chain shared by adapters.

use std::fmt;
use std::sync::Arc;

use chigiri_core::GraphQlRequest;

use crate::{
    AdmissionDecision, AdmissionError, DecodedRequestHook, DecodedRequestView,
    OperationSourceResolver, RequestEnvelope, SourceResolutionError, TransportAdmission,
    TransportRequest,
};

/// Immutable configuration for transport, source, and decoded phases.
#[derive(Clone, Default)]
pub struct HttpLifecycle {
    transport: Vec<Arc<dyn TransportAdmission>>,
    source: Option<Arc<dyn OperationSourceResolver>>,
    decoded: Vec<Arc<dyn DecodedRequestHook>>,
}

impl HttpLifecycle {
    /// Creates a lifecycle with no interceptors or source resolver.
    pub const fn new() -> Self {
        Self {
            transport: Vec::new(),
            source: None,
            decoded: Vec::new(),
        }
    }

    /// Appends an interceptor that runs before body polling.
    pub fn transport_admission(mut self, admission: impl TransportAdmission) -> Self {
        self.transport.push(Arc::new(admission));
        self
    }

    /// Sets the operation-source resolver used after envelope decoding.
    pub fn operation_source(mut self, resolver: impl OperationSourceResolver) -> Self {
        self.source = Some(Arc::new(resolver));
        self
    }

    /// Appends a read-only interceptor that runs after source resolution.
    pub fn decoded_request(mut self, hook: impl DecodedRequestHook) -> Self {
        self.decoded.push(Arc::new(hook));
        self
    }

    /// The adapter calls the transport phase only once.
    #[doc(hidden)]
    pub async fn admit_transport(
        &self,
        request: TransportRequest<'_>,
    ) -> Result<AdmissionDecision, AdmissionError> {
        for admission in &self.transport {
            match admission.admit(request).await? {
                AdmissionDecision::Continue => {}
                rejection @ AdmissionDecision::Reject(_) => return Ok(rejection),
            }
        }
        Ok(AdmissionDecision::Continue)
    }

    /// The adapter only calls the source phase once.
    #[doc(hidden)]
    pub async fn resolve_source(
        &self,
        envelope: RequestEnvelope,
    ) -> Result<GraphQlRequest, SourceResolutionError> {
        let source = match &self.source {
            Some(resolver) => resolver.resolve(&envelope).await?,
            None => envelope
                .document_text()
                .map(crate::ResolvedOperationSource::new)
                .ok_or_else(missing_document)?,
        };
        let cost_hint = source.catalog_cost_hint();
        let request = envelope.resolve(source.into_document());
        Ok(match cost_hint {
            Some(cost) => request.with_catalog_cost_hint(cost),
            None => request,
        })
    }

    /// The adapter calls the decoded phase once.
    #[doc(hidden)]
    pub async fn inspect_decoded(
        &self,
        request: DecodedRequestView<'_>,
    ) -> Result<AdmissionDecision, AdmissionError> {
        for hook in &self.decoded {
            match hook.inspect(request).await? {
                AdmissionDecision::Continue => {}
                rejection @ AdmissionDecision::Reject(_) => return Ok(rejection),
            }
        }
        Ok(AdmissionDecision::Continue)
    }
}

impl fmt::Debug for HttpLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpLifecycle")
            .field("transport_admission_count", &self.transport.len())
            .field("has_operation_source", &self.source.is_some())
            .field("decoded_request_count", &self.decoded.len())
            .finish()
    }
}

fn missing_document() -> SourceResolutionError {
    SourceResolutionError::Internal(AdmissionError::new(MissingDocument))
}

#[derive(Debug)]
struct MissingDocument;

impl fmt::Display for MissingDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation source was not resolved")
    }
}

impl std::error::Error for MissingDocument {}
