use std::sync::{Arc, Barrier, Mutex};

use chigiri_operation::{
    OperationAuditEvent, OperationAuditOutcome, OperationAuditSink, OperationAuditSinkError,
    OperationRegistry, OperationSourcePolicy, PersistedOperationId, ProviderError,
    RegistryDocument,
};
use chigiri_runtime::{
    DocumentDigest, ErrorEvent, ErrorObserver, GraphQlRequest, RequestId, RequestTelemetry,
    SamplingDecision, TraceId,
};
use sha2::{Digest as _, Sha256};

pub const PROVIDER_MARKER: &str = "provider-source-marker";
pub const SINK_MARKER: &str = "sink-source-marker";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub sampling: SamplingDecision,
    pub has_operation: bool,
    pub document_digest: String,
    pub catalog_revision: Option<String>,
    pub mode: OperationSourcePolicy,
    pub outcome: OperationAuditOutcome,
    pub event_debug: String,
    pub identity_debug: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorRecord {
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub sampling: SamplingDecision,
    pub has_operation: bool,
    pub code: &'static str,
    pub component: &'static str,
    pub event_debug: String,
}

pub struct Registry {
    document: Option<Arc<str>>,
    fails: bool,
    barrier: Option<Arc<Barrier>>,
}

impl Registry {
    pub fn hit(document: &str) -> Self {
        Self {
            document: Some(Arc::from(document)),
            fails: false,
            barrier: None,
        }
    }

    pub const fn miss() -> Self {
        Self {
            document: None,
            fails: false,
            barrier: None,
        }
    }

    pub const fn failing() -> Self {
        Self {
            document: None,
            fails: true,
            barrier: None,
        }
    }

    pub fn with_barrier(mut self, barrier: Arc<Barrier>) -> Self {
        self.barrier = Some(barrier);
        self
    }

    fn result(
        &self,
    ) -> chigiri_http::BoxFuture<'_, Result<Option<RegistryDocument>, ProviderError>> {
        if let Some(barrier) = &self.barrier {
            // Start both lookups from independent threads so the competing request
            // does not depend on scheduler order.
            barrier.wait();
        }
        if self.fails {
            return Box::pin(async {
                Err(ProviderError::new(std::io::Error::other(PROVIDER_MARKER)))
            });
        }
        let document = self.document.clone().map(RegistryDocument::new);
        Box::pin(async move { Ok(document) })
    }
}

impl OperationRegistry for Registry {
    fn lookup<'a>(
        &'a self,
        _: &'a PersistedOperationId,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        self.result()
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        _: &'a DocumentDigest,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        self.result()
    }

    fn revision(&self) -> Option<&str> {
        Some("audit-revision")
    }
}

#[derive(Clone, Default)]
pub struct RecordingAuditSink {
    records: Arc<Mutex<Vec<AuditRecord>>>,
    fails: bool,
}

impl RecordingAuditSink {
    pub fn failing() -> Self {
        Self {
            records: Arc::default(),
            fails: true,
        }
    }

    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().unwrap().clone()
    }
}

impl OperationAuditSink for RecordingAuditSink {
    fn record<'a>(
        &'a self,
        event: OperationAuditEvent<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<(), OperationAuditSinkError>> {
        let identity = event.identity();
        self.records.lock().unwrap().push(AuditRecord {
            request_id: identity.request_id().map(|id| id.as_str().to_owned()),
            trace_id: identity.trace_id().map(|id| id.as_str().to_owned()),
            sampling: identity.sampling(),
            has_operation: identity.operation().is_some(),
            document_digest: event.document_digest().to_hex(),
            catalog_revision: event.catalog_revision().map(str::to_owned),
            mode: event.mode(),
            outcome: event.outcome(),
            event_debug: format!("{event:?}"),
            identity_debug: format!("{identity:?}"),
        });
        let fails = self.fails;
        Box::pin(async move {
            if fails {
                Err(OperationAuditSinkError::new(std::io::Error::other(
                    SINK_MARKER,
                )))
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Clone, Default)]
pub struct RecordingErrorObserver {
    records: Arc<Mutex<Vec<ErrorRecord>>>,
}

impl RecordingErrorObserver {
    pub fn records(&self) -> Vec<ErrorRecord> {
        self.records.lock().unwrap().clone()
    }
}

impl ErrorObserver for RecordingErrorObserver {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        let identity = event.identity();
        self.records.lock().unwrap().push(ErrorRecord {
            request_id: identity.request_id().map(|id| id.as_str().to_owned()),
            trace_id: identity.trace_id().map(|id| id.as_str().to_owned()),
            sampling: identity.sampling(),
            has_operation: identity.operation().is_some(),
            code: event.code(),
            component: event.component(),
            event_debug: format!("{event:?}"),
        });
    }
}

pub fn envelope(
    document: &str,
    request_id: &str,
    trace_id: &str,
    sampling: SamplingDecision,
) -> chigiri_http::RequestEnvelope {
    let telemetry = RequestTelemetry::new(
        Some(RequestId::new(request_id).unwrap()),
        Some(TraceId::new(trace_id).unwrap()),
        sampling,
    );
    chigiri_http::RequestEnvelope::document(GraphQlRequest::new(document))
        .with_request_telemetry(telemetry)
}

pub fn assert_identity(
    record: &AuditRecord,
    request_id: &str,
    trace_id: &str,
    sampling: SamplingDecision,
) {
    assert_eq!(record.request_id.as_deref(), Some(request_id));
    assert_eq!(record.trace_id.as_deref(), Some(trace_id));
    assert_eq!(record.sampling, sampling);
    assert!(!record.has_operation);
}

pub fn assert_error_identity(
    error: &ErrorRecord,
    request_id: &str,
    trace_id: &str,
    sampling: SamplingDecision,
) {
    assert_eq!(error.request_id.as_deref(), Some(request_id));
    assert_eq!(error.trace_id.as_deref(), Some(trace_id));
    assert_eq!(error.sampling, sampling);
    assert!(!error.has_operation);
}

pub fn assert_error_matches(audit: &AuditRecord, error: &ErrorRecord) {
    assert_eq!(error.request_id, audit.request_id);
    assert_eq!(error.trace_id, audit.trace_id);
    assert_eq!(error.sampling, audit.sampling);
    assert_eq!(error.has_operation, audit.has_operation);
}

pub fn assert_empty_identity(audit: &AuditRecord, error: &ErrorRecord) {
    assert_eq!(audit.request_id, None);
    assert_eq!(audit.trace_id, None);
    assert_eq!(audit.sampling, SamplingDecision::Record);
    assert!(!audit.has_operation);
    assert_eq!(error.request_id, None);
    assert_eq!(error.trace_id, None);
    assert_eq!(error.sampling, SamplingDecision::Record);
    assert!(!error.has_operation);
}

pub fn digest(document: &str) -> String {
    format!("{:x}", Sha256::digest(document.as_bytes()))
}
