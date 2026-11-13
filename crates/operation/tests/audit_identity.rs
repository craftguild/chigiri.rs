#[path = "audit_identity/support.rs"]
mod support;

use std::sync::{Arc, Barrier};

use chigiri_http::{OperationSourceResolver, RequestEnvelope};
use chigiri_operation::{
    OperationAuditOutcome, OperationAuditSinkError, OperationSourcePolicy,
    PersistedOperationResolver, ProviderError,
};
use chigiri_runtime::{GraphQlRequest, SamplingDecision};

use support::{
    PROVIDER_MARKER, RecordingAuditSink, RecordingErrorObserver, Registry, SINK_MARKER,
    assert_empty_identity, assert_error_identity, assert_error_matches, assert_identity, digest,
    envelope,
};

#[tokio::test]
async fn hit_identity() {
    let document = "query AuditHit { value }";
    let sink = RecordingAuditSink::default();
    let resolver = PersistedOperationResolver::audit(
        Registry::hit(document),
        sink.clone(),
        RecordingErrorObserver::default(),
    );
    let source = resolver
        .resolve(&envelope(
            document,
            "request-hit",
            "trace-hit",
            SamplingDecision::Record,
        ))
        .await
        .unwrap();

    assert_eq!(source.into_document(), document);
    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert_identity(
        &records[0],
        "request-hit",
        "trace-hit",
        SamplingDecision::Record,
    );
    assert_eq!(records[0].outcome, OperationAuditOutcome::Hit);
}

#[tokio::test]
async fn miss_identity() {
    let document = "query AuditMiss { value }";
    let sink = RecordingAuditSink::default();
    let resolver = PersistedOperationResolver::audit(
        Registry::miss(),
        sink.clone(),
        RecordingErrorObserver::default(),
    );
    resolver
        .resolve(&envelope(
            document,
            "request-miss",
            "trace-miss",
            SamplingDecision::Record,
        ))
        .await
        .unwrap();

    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert_identity(
        &records[0],
        "request-miss",
        "trace-miss",
        SamplingDecision::Record,
    );
    assert_eq!(records[0].document_digest, digest(document));
    assert_eq!(
        records[0].catalog_revision.as_deref(),
        Some("audit-revision")
    );
    assert_eq!(records[0].mode, OperationSourcePolicy::Audit);
    assert_eq!(records[0].outcome, OperationAuditOutcome::Miss);
}

#[tokio::test]
async fn provider_failure_identity() {
    let document = "query ProviderFailure { value }";
    let sink = RecordingAuditSink::default();
    let observer = RecordingErrorObserver::default();
    let resolver =
        PersistedOperationResolver::audit(Registry::failing(), sink.clone(), observer.clone());
    let source = resolver
        .resolve(&envelope(
            document,
            "request-provider",
            "trace-provider",
            SamplingDecision::Drop,
        ))
        .await
        .unwrap();

    assert_eq!(source.into_document(), document);
    let audits = sink.records();
    let errors = observer.records();
    assert_eq!(audits.len(), 1);
    assert_eq!(errors.len(), 1);
    assert_eq!(
        audits[0].outcome,
        OperationAuditOutcome::ProviderUnavailable
    );
    assert_identity(
        &audits[0],
        "request-provider",
        "trace-provider",
        SamplingDecision::Drop,
    );
    assert_error_identity(
        &errors[0],
        "request-provider",
        "trace-provider",
        SamplingDecision::Drop,
    );
    assert_error_matches(&audits[0], &errors[0]);
    assert_eq!(errors[0].code, "OPERATION_AUDIT_PROVIDER_FAILED");
    assert_eq!(errors[0].component, "operation-registry");
}

#[tokio::test]
async fn sink_failure_identity() {
    let document = "query SinkFailure { value }";
    let sink = RecordingAuditSink::failing();
    let observer = RecordingErrorObserver::default();
    let resolver =
        PersistedOperationResolver::audit(Registry::hit(document), sink.clone(), observer.clone());
    let source = resolver
        .resolve(&envelope(
            document,
            "request-sink",
            "trace-sink",
            SamplingDecision::Record,
        ))
        .await
        .unwrap();

    assert_eq!(source.into_document(), document);
    let audits = sink.records();
    let errors = observer.records();
    assert_eq!(audits.len(), 1);
    assert_eq!(errors.len(), 1);
    assert_identity(
        &audits[0],
        "request-sink",
        "trace-sink",
        SamplingDecision::Record,
    );
    assert_error_identity(
        &errors[0],
        "request-sink",
        "trace-sink",
        SamplingDecision::Record,
    );
    assert_error_matches(&audits[0], &errors[0]);
    assert_eq!(errors[0].code, "OPERATION_AUDIT_SINK_FAILED");
    assert_eq!(errors[0].component, "operation-audit-sink");
}

#[tokio::test]
async fn missing_telemetry() {
    let document = "query MissingTelemetry { value }";
    let sink = RecordingAuditSink::default();
    let resolver = PersistedOperationResolver::audit(
        Registry::miss(),
        sink.clone(),
        RecordingErrorObserver::default(),
    );
    resolver
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(document)))
        .await
        .unwrap();

    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].request_id, None);
    assert_eq!(records[0].trace_id, None);
    assert!(!records[0].has_operation);
    assert_eq!(records[0].sampling, SamplingDecision::Record);

    let provider_sink = RecordingAuditSink::default();
    let provider_errors = RecordingErrorObserver::default();
    let provider = PersistedOperationResolver::audit(
        Registry::failing(),
        provider_sink.clone(),
        provider_errors.clone(),
    );
    let source = provider
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(document)))
        .await
        .unwrap();
    assert_eq!(source.into_document(), document);
    let provider_audits = provider_sink.records();
    let provider_errors = provider_errors.records();
    assert_eq!(provider_audits.len(), 1);
    assert_eq!(provider_errors.len(), 1);
    assert_empty_identity(&provider_audits[0], &provider_errors[0]);
    assert_eq!(
        provider_audits[0].outcome,
        OperationAuditOutcome::ProviderUnavailable
    );
    assert_eq!(provider_errors[0].code, "OPERATION_AUDIT_PROVIDER_FAILED");

    let failing_sink = RecordingAuditSink::failing();
    let sink_errors = RecordingErrorObserver::default();
    let sink_failure = PersistedOperationResolver::audit(
        Registry::hit(document),
        failing_sink.clone(),
        sink_errors.clone(),
    );
    let source = sink_failure
        .resolve(&RequestEnvelope::document(GraphQlRequest::new(document)))
        .await
        .unwrap();
    assert_eq!(source.into_document(), document);
    let sink_audits = failing_sink.records();
    let sink_errors = sink_errors.records();
    assert_eq!(sink_audits.len(), 1);
    assert_eq!(sink_errors.len(), 1);
    assert_empty_identity(&sink_audits[0], &sink_errors[0]);
    assert_eq!(sink_errors[0].code, "OPERATION_AUDIT_SINK_FAILED");
}

#[tokio::test]
async fn sampling_drop() {
    let document = "query SamplingDrop { value }";
    let sink = RecordingAuditSink::default();
    let resolver = PersistedOperationResolver::audit(
        Registry::miss(),
        sink.clone(),
        RecordingErrorObserver::default(),
    );
    resolver
        .resolve(&envelope(
            document,
            "request-drop",
            "trace-drop",
            SamplingDecision::Drop,
        ))
        .await
        .unwrap();

    let records = sink.records();
    assert_eq!(records.len(), 1);
    assert_identity(
        &records[0],
        "request-drop",
        "trace-drop",
        SamplingDecision::Drop,
    );
}

#[test]
fn parallel_identity() {
    let sink = RecordingAuditSink::default();
    let resolver = Arc::new(PersistedOperationResolver::audit(
        Registry::miss().with_barrier(Arc::new(Barrier::new(2))),
        sink.clone(),
        RecordingErrorObserver::default(),
    ));
    let cases = [
        ("query ParallelOne { value }", "request-one", "trace-one"),
        ("query ParallelTwo { value }", "request-two", "trace-two"),
    ];
    let handles = cases.map(|(document, request_id, trace_id)| {
        let resolver = Arc::clone(&resolver);
        std::thread::spawn(move || {
            let envelope = envelope(document, request_id, trace_id, SamplingDecision::Record);
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(async { resolver.resolve(&envelope).await.unwrap() });
        })
    });
    for handle in handles {
        handle.join().unwrap();
    }

    let mut records = sink.records();
    records.sort_by(|left, right| left.request_id.cmp(&right.request_id));
    assert_eq!(records.len(), 2);
    for ((document, request_id, trace_id), record) in cases.into_iter().zip(records) {
        assert_identity(&record, request_id, trace_id, SamplingDecision::Record);
        assert_eq!(record.document_digest, digest(document));
        assert_eq!(record.outcome, OperationAuditOutcome::Miss);
    }
}

#[tokio::test]
async fn redaction() {
    let document = "query RedactionDocumentMarker { value }";
    let request_id = "request-redaction-marker";
    let trace_id = "trace-redaction-marker";
    let sink = RecordingAuditSink::failing();
    let observer = RecordingErrorObserver::default();
    let resolver =
        PersistedOperationResolver::audit(Registry::failing(), sink.clone(), observer.clone());
    let source = resolver
        .resolve(&envelope(
            document,
            request_id,
            trace_id,
            SamplingDecision::Drop,
        ))
        .await
        .unwrap();

    assert!(!format!("{source:?}").contains("RedactionDocumentMarker"));
    let audits = sink.records();
    let errors = observer.records();
    assert_eq!(audits.len(), 1);
    assert_eq!(errors.len(), 2);
    let debug = format!(
        "{}{}{}{}",
        audits[0].event_debug,
        audits[0].identity_debug,
        errors[0].event_debug,
        errors[1].event_debug
    );
    for hidden in [request_id, trace_id, document, PROVIDER_MARKER, SINK_MARKER] {
        assert!(!debug.contains(hidden));
    }
    let provider = ProviderError::new(std::io::Error::other(PROVIDER_MARKER));
    let sink_error = OperationAuditSinkError::new(std::io::Error::other(SINK_MARKER));
    assert!(!format!("{provider:?}").contains(PROVIDER_MARKER));
    assert!(!format!("{sink_error:?}").contains(SINK_MARKER));
}
