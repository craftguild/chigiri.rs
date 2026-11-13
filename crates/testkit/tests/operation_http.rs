#[path = "lifecycle_support.rs"]
pub mod lifecycle_support;

use std::fmt::Write as _;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use chigiri::{
    DocumentDigest, Endpoint, RequestId, RequestIdFactory, RequestIdFactoryError, SamplingDecision,
    TraceId,
};
use chigiri_http::{BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext};
use chigiri_observability::LifecycleExporter;
use chigiri_operation::{
    CatalogEntry, CatalogLimits, CatalogRevision, InMemoryApqStore, InMemoryOperationCatalog,
    OperationAuditEvent, OperationAuditOutcome, OperationAuditSink, OperationAuditSinkError,
    OperationRegistry, OperationSourcePolicy, PersistedOperationId, PersistedOperationResolver,
    ProviderError, RegistryDocument, StoreLimits,
};
use http_body_util::BodyExt as _;
use sha2::{Digest as _, Sha256};

use lifecycle_support::{PhaseLog, TestContext, endpoint_builder};

const REQUEST_MARKER: &str = "request-http-redaction-marker";
const TRACE_MARKER: &str = "trace-http-redaction-marker";
const DOCUMENT_MARKER: &str = "RawHttpDocumentMarker";
const PROVIDER_MARKER: &str = "provider-http-source-marker";
const SINK_MARKER: &str = "sink-http-source-marker";

struct Factory;

impl ContextFactory<TestContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        Box::pin(async {
            Ok(PreparedContext::execution_only(TestContext {
                log: PhaseLog::default(),
            }))
        })
    }
}

#[tokio::test]
async fn op_04_to_06_run_apq_safelist_id_only_from_http_envelope() {
    let document = "{ hello }";
    let hash = format!("{:x}", Sha256::digest(document.as_bytes()));

    let apq = handler(PersistedOperationResolver::apq(InMemoryApqStore::new(
        StoreLimits::default(),
    )));
    let (status, body) = execute(&apq, persisted_body(None, &hash)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(code(&body), Some("PERSISTED_OPERATION_NOT_FOUND"));

    let empty = InMemoryOperationCatalog::new(
        CatalogRevision::new("empty").unwrap(),
        Vec::new(),
        CatalogLimits::default(),
    )
    .unwrap();
    let safelist = handler(PersistedOperationResolver::safelist(empty));
    let (status, body) = execute(&safelist, persisted_body(Some(document), &hash)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&body), Some("PERSISTED_OPERATION_NOT_ALLOWED"));

    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("known").unwrap(),
        vec![CatalogEntry::new(
            PersistedOperationId::new(&hash).unwrap(),
            document,
        )],
        CatalogLimits::default(),
    )
    .unwrap();
    let id_only = handler(PersistedOperationResolver::id_only(catalog));
    let (status, body) = execute(&id_only, persisted_body(None, &hash)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["hello"], "ok");
}

#[tokio::test]
async fn op_02_oversized_operation_identity_returns_a_bounded_error_on_axum() {
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .context_factory(Factory)
        .build()
        .unwrap();
    let name = "a".repeat(256);
    let body = serde_json::json!({
        "query": format!("query {name} {{ hello }}"),
        "operationName": name,
    });
    let (status, body) = execute(&endpoint, serde_json::to_string(&body).unwrap()).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&body), Some("GRAPHQL_VALIDATION_FAILED"));
}

#[tokio::test]
async fn audit_id_08_axum_response_and_standard_diagnostic_are_redacted() {
    let sink = FailingAuditSink::default();
    let (diagnostics, _lifecycle, errors) = LifecycleExporter::new(4).unwrap();
    let retained_diagnostics = diagnostics.clone();
    let resolver =
        PersistedOperationResolver::audit(FailingRegistry, sink.clone(), diagnostics.clone());
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .operation_source(resolver)
        .inject(|components| {
            components
                .observability()
                .request_id_factory(FixedRequestIds)
                .error_observer(diagnostics);
        })
        .context_factory(Factory)
        .build()
        .unwrap();
    let document = format!("query {DOCUMENT_MARKER} {{ hello }}");
    let mut request = post(serde_json::json!({"query": document}).to_string());
    request
        .extensions_mut()
        .insert(TraceId::new(TRACE_MARKER).unwrap());

    let response = chigiri_axum::handler(endpoint)(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/graphql-response+json; charset=utf-8"
    );
    let mut header_surface = String::new();
    for (name, value) in response.headers() {
        write!(
            &mut header_surface,
            "{name}:{}",
            String::from_utf8_lossy(value.as_bytes())
        )
        .unwrap();
    }
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), br#"{"data":{"hello":"ok"}}"#);

    let audits = sink.records.lock().unwrap();
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.request_id.as_deref(), Some(REQUEST_MARKER));
    assert_eq!(audit.trace_id.as_deref(), Some(TRACE_MARKER));
    assert_eq!(audit.sampling, SamplingDecision::Record);
    assert!(!audit.has_operation);
    assert_eq!(audit.mode, OperationSourcePolicy::Audit);
    assert_eq!(audit.outcome, OperationAuditOutcome::ProviderUnavailable);

    let mut diagnostic_records = vec![errors.try_recv().unwrap(), errors.try_recv().unwrap()];
    assert_eq!(errors.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    assert_eq!(retained_diagnostics.dropped_errors(), 0);
    diagnostic_records.sort_by_key(|record| record.code);
    assert_eq!(
        diagnostic_records
            .iter()
            .map(|record| (record.code, record.component))
            .collect::<Vec<_>>(),
        vec![
            ("OPERATION_AUDIT_PROVIDER_FAILED", "operation-registry"),
            ("OPERATION_AUDIT_SINK_FAILED", "operation-audit-sink"),
        ]
    );
    assert!(
        diagnostic_records
            .iter()
            .all(|record| record.has_request_id && !record.has_operation)
    );
    let public_surface = format!(
        "{}{}{}{}{}",
        StatusCode::OK,
        header_surface,
        String::from_utf8_lossy(&body),
        audit.debug,
        format_args!("{diagnostic_records:?}")
    );
    for marker in [
        REQUEST_MARKER,
        TRACE_MARKER,
        DOCUMENT_MARKER,
        PROVIDER_MARKER,
        SINK_MARKER,
    ] {
        assert!(!public_surface.contains(marker));
    }
}

fn handler(resolver: PersistedOperationResolver) -> Endpoint<TestContext> {
    endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .operation_source(resolver)
        .context_factory(Factory)
        .build()
        .unwrap()
}

fn post(body: String) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(Body::from(body))
        .unwrap()
}

async fn execute(
    endpoint: &Endpoint<TestContext>,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(Body::from(body))
        .unwrap();
    let response = chigiri_axum::handler(endpoint.clone())(request).await;
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

fn persisted_body(document: Option<&str>, hash: &str) -> String {
    let mut request = serde_json::json!({
        "extensions": {
            "persistedQuery": {"version": 1, "sha256Hash": hash}
        }
    });
    if let Some(document) = document {
        request["query"] = document.into();
    }
    serde_json::to_string(&request).unwrap()
}

fn code(body: &serde_json::Value) -> Option<&str> {
    body["errors"][0]["extensions"]["code"].as_str()
}

struct FailingRegistry;

impl OperationRegistry for FailingRegistry {
    fn lookup<'a>(
        &'a self,
        _: &'a PersistedOperationId,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        failing_lookup()
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        _: &'a DocumentDigest,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        failing_lookup()
    }

    fn revision(&self) -> Option<&str> {
        Some("http-redaction-revision")
    }
}

fn failing_lookup<'a>() -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
    Box::pin(async { Err(ProviderError::new(std::io::Error::other(PROVIDER_MARKER))) })
}

#[derive(Clone, Default)]
struct FailingAuditSink {
    records: Arc<Mutex<Vec<AuditSnapshot>>>,
}

struct AuditSnapshot {
    request_id: Option<String>,
    trace_id: Option<String>,
    sampling: SamplingDecision,
    has_operation: bool,
    mode: OperationSourcePolicy,
    outcome: OperationAuditOutcome,
    debug: String,
}

impl OperationAuditSink for FailingAuditSink {
    fn record<'a>(
        &'a self,
        event: OperationAuditEvent<'a>,
    ) -> BoxFuture<'a, Result<(), OperationAuditSinkError>> {
        let identity = event.identity();
        self.records.lock().unwrap().push(AuditSnapshot {
            request_id: identity.request_id().map(|id| id.as_str().to_owned()),
            trace_id: identity.trace_id().map(|id| id.as_str().to_owned()),
            sampling: identity.sampling(),
            has_operation: identity.operation().is_some(),
            mode: event.mode(),
            outcome: event.outcome(),
            debug: format!("{event:?}{identity:?}"),
        });
        Box::pin(async {
            Err(OperationAuditSinkError::new(std::io::Error::other(
                SINK_MARKER,
            )))
        })
    }
}

#[derive(Clone, Copy)]
struct FixedRequestIds;

impl RequestIdFactory for FixedRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        RequestId::new(REQUEST_MARKER).map_err(RequestIdFactoryError::new)
    }
}
