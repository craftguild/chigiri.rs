use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{
    OperationSourceResolver, PersistedOperationReference, RequestEnvelope, SourceResolutionError,
};
use chigiri_operation::{
    ApqStore, CatalogEntry, CatalogLimits, CatalogRevision, InMemoryOperationCatalog,
    OperationAuditEvent, OperationAuditOutcome, OperationAuditSink, OperationAuditSinkError,
    OperationRegistry, OperationSourcePolicy, PersistedOperationId, PersistedOperationResolver,
    ProviderError, RegistryDocument,
};
use chigiri_runtime::{DocumentDigest, ErrorEvent, ErrorObserver, GraphQlRequest, Object};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Default)]
struct Store {
    documents: Arc<Mutex<BTreeMap<PersistedOperationId, Arc<str>>>>,
    puts: Arc<AtomicUsize>,
}

impl OperationRegistry for Store {
    fn lookup<'a>(
        &'a self,
        id: &'a PersistedOperationId,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            Ok(self
                .documents
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .map(RegistryDocument::new))
        })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        digest: &'a DocumentDigest,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            let id = PersistedOperationId::new(digest.to_hex()).unwrap();
            self.lookup(&id).await
        })
    }
}

impl ApqStore for Store {
    fn put(
        &self,
        id: PersistedOperationId,
        document: Arc<str>,
    ) -> chigiri_http::BoxFuture<'_, Result<(), ProviderError>> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            self.documents.lock().unwrap().insert(id, document);
            Ok(())
        })
    }
}

#[tokio::test]
async fn op_04_apq_hash_mismatch_is_rejected_before_store_write() {
    let store = Store::default();
    let resolver = PersistedOperationResolver::apq(store.clone());
    let envelope = persisted(Some("{ value }"), &"0".repeat(64));
    let error = resolver.resolve(&envelope).await.unwrap_err();
    assert_rejection(error, 400, "PERSISTED_OPERATION_HASH_MISMATCH");
    assert_eq!(store.puts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn op_05_apq_unknown_and_safelist_unknown_return_different_results() {
    let document = "{ value }";
    let hash = hash(document);
    let apq = PersistedOperationResolver::apq(Store::default());
    let error = apq.resolve(&persisted(None, &hash)).await.unwrap_err();
    assert!(matches!(error, SourceResolutionError::Protocol(_)));

    let safelist = PersistedOperationResolver::safelist(Store::default());
    let error = safelist
        .resolve(&persisted(Some(document), &hash))
        .await
        .unwrap_err();
    assert_rejection(error, 403, "PERSISTED_OPERATION_NOT_ALLOWED");
}

#[tokio::test]
async fn op_05_safelist_compares_opaque_alias_with_raw_document_digest() {
    let document = "query Books { value }";
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("revision-1").unwrap(),
        vec![
            CatalogEntry::new(PersistedOperationId::new("books-v1").unwrap(), document)
                .with_cost_hint(7)
                .unwrap(),
        ],
        CatalogLimits::default(),
    )
    .unwrap();
    let resolver = PersistedOperationResolver::safelist(catalog);
    let raw = RequestEnvelope::document(GraphQlRequest::new(document));
    let raw = resolver.resolve(&raw).await.unwrap();
    assert_eq!(raw.catalog_cost_hint(), Some(7));
    assert_eq!(raw.into_document(), document);
    let opaque = RequestEnvelope::persisted(
        Some(document.to_owned()),
        None,
        Object::new(),
        Object::new(),
        PersistedOperationReference::opaque("books-v1").unwrap(),
    );
    assert_eq!(
        resolver.resolve(&opaque).await.unwrap().into_document(),
        document
    );
}

#[tokio::test]
async fn op_06_id_only_rejects_text_and_resolves_known_ids_to_exact_documents() {
    let document = "query Exact { value }";
    let hash = hash(document);
    let store = Store::default();
    store.documents.lock().unwrap().insert(
        PersistedOperationId::new(&hash).unwrap(),
        Arc::from(document),
    );
    let resolver = PersistedOperationResolver::id_only(store);
    let with_text = resolver
        .resolve(&persisted(Some(document), &hash))
        .await
        .unwrap_err();
    assert_rejection(with_text, 403, "PERSISTED_OPERATION_NOT_ALLOWED");
    assert_eq!(
        resolver
            .resolve(&persisted(None, &hash))
            .await
            .unwrap()
            .into_document(),
        document
    );
}

#[tokio::test]
async fn op_07_provider_timeout_drops_future_and_preserves_other_failure_policies() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let resolver = PersistedOperationResolver::safelist(PendingRegistry(Arc::clone(&dropped)));
    let document = "{ value }";
    let envelope = persisted(Some(document), &hash(document));
    assert!(
        tokio::time::timeout(Duration::from_millis(5), resolver.resolve(&envelope))
            .await
            .is_err()
    );
    assert!(dropped.load(Ordering::SeqCst));

    let unavailable = PersistedOperationResolver::id_only(FailingRegistry)
        .resolve(&persisted(None, &"1".repeat(64)))
        .await
        .unwrap_err();
    assert!(matches!(unavailable, SourceResolutionError::Unavailable(_)));
    let audit_document = "{ value }";
    let audit = PersistedOperationResolver::audit(
        FailingRegistry,
        RecordingAuditSink::default(),
        RecordingErrorObserver::default(),
    )
    .resolve(&persisted(Some(audit_document), &hash(audit_document)))
    .await
    .unwrap();
    assert_eq!(audit.into_document(), audit_document);
}

#[tokio::test]
async fn audit_records_digest_revision_hit_and_miss_without_exposing_queries_and_reports_sink_failure()
 {
    let document = "query Books { value }";
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("revision-1").unwrap(),
        vec![CatalogEntry::new(
            PersistedOperationId::new("books-v1").unwrap(),
            document,
        )],
        CatalogLimits::default(),
    )
    .unwrap();
    let sink = RecordingAuditSink::default();
    let resolver =
        PersistedOperationResolver::audit(catalog, sink.clone(), RecordingErrorObserver::default());
    resolver
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(document)))
        .await
        .unwrap();
    resolver
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(
            "{ unknown }",
        )))
        .await
        .unwrap();
    {
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, hash(document));
        assert_eq!(events[0].1.as_deref(), Some("revision-1"));
        assert_eq!(events[0].2, OperationSourcePolicy::Audit);
        assert_eq!(events[0].3, OperationAuditOutcome::Hit);
        assert_eq!(events[1].3, OperationAuditOutcome::Miss);
    }

    let observer = RecordingErrorObserver::default();
    let resolver =
        PersistedOperationResolver::audit(Store::default(), FailingAuditSink, observer.clone());
    resolver
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(document)))
        .await
        .unwrap();
    assert_eq!(observer.count.load(Ordering::SeqCst), 1);
}

#[test]
fn op_08_debug_and_public_errors_do_not_expose_raw_queries_ids_or_provider_sources() {
    let secret = "query SecretOperation { value }";
    let envelope = persisted(Some(secret), &hash(secret));
    assert!(!format!("{envelope:?}").contains("SecretOperation"));
    let provider = ProviderError::new(std::io::Error::other("provider-secret"));
    assert!(!format!("{provider:?}").contains("provider-secret"));
    assert!(!provider.to_string().contains("provider-secret"));
    let id = PersistedOperationId::new("private-catalog-id").unwrap();
    assert!(!format!("{id:?}").contains("private-catalog-id"));
}

struct PendingRegistry(Arc<std::sync::atomic::AtomicBool>);

impl OperationRegistry for PendingRegistry {
    fn lookup<'a>(
        &'a self,
        _: &'a PersistedOperationId,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(PendingProvider {
            dropped: Arc::clone(&self.0),
        })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        _: &'a DocumentDigest,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(PendingProvider {
            dropped: Arc::clone(&self.0),
        })
    }
}

struct PendingProvider {
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

impl Future for PendingProvider {
    type Output = Result<Option<RegistryDocument>, ProviderError>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingProvider {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

struct FailingRegistry;

impl OperationRegistry for FailingRegistry {
    fn lookup<'a>(
        &'a self,
        _: &'a PersistedOperationId,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async { Err(ProviderError::new(std::io::Error::other("provider-secret"))) })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        _: &'a DocumentDigest,
    ) -> chigiri_http::BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async { Err(ProviderError::new(std::io::Error::other("provider-secret"))) })
    }
}

#[derive(Clone, Default)]
struct RecordingAuditSink {
    events: Arc<Mutex<Vec<AuditRecord>>>,
}

type AuditRecord = (
    String,
    Option<String>,
    OperationSourcePolicy,
    OperationAuditOutcome,
);

impl OperationAuditSink for RecordingAuditSink {
    fn record<'a>(
        &'a self,
        event: OperationAuditEvent<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<(), OperationAuditSinkError>> {
        self.events.lock().unwrap().push((
            event.document_digest().to_hex(),
            event.catalog_revision().map(str::to_owned),
            event.mode(),
            event.outcome(),
        ));
        Box::pin(async { Ok(()) })
    }
}

struct FailingAuditSink;

impl OperationAuditSink for FailingAuditSink {
    fn record<'a>(
        &'a self,
        _: OperationAuditEvent<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<(), OperationAuditSinkError>> {
        Box::pin(async {
            Err(OperationAuditSinkError::new(std::io::Error::other(
                "sink-secret",
            )))
        })
    }
}

#[derive(Clone, Default)]
struct RecordingErrorObserver {
    count: Arc<AtomicUsize>,
}

impl ErrorObserver for RecordingErrorObserver {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

fn persisted(document: Option<&str>, hash: &str) -> RequestEnvelope {
    RequestEnvelope::persisted(
        document.map(str::to_owned),
        None,
        Object::new(),
        GraphQlRequest::new("").extensions,
        PersistedOperationReference::apollo_apq_v1(hash).unwrap(),
    )
}

fn hash(document: &str) -> String {
    format!("{:x}", Sha256::digest(document.as_bytes()))
}

fn assert_rejection(error: SourceResolutionError, status: u16, code: &str) {
    let SourceResolutionError::Reject(rejection) = error else {
        panic!("expected a typed rejection.");
    };
    assert_eq!(rejection.status(), status);
    assert_eq!(
        rejection
            .error()
            .extensions
            .get("code")
            .and_then(|value| value.as_str()),
        Some(code)
    );
}
