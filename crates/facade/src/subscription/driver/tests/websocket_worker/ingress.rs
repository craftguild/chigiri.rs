mod probe;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_http::{
    ApplicationMetadata, HttpLifecycle, RequestEnvelope, RequestHeader, RequestMethod,
    ResponseMediaType, TransportRequest,
};
use chigiri_runtime::{GraphQlRequest, Value};

use super::super::harness;
use super::super::state::{State, count};
use super::support::{Registered, TokioDeadlines, echo_request, run, setup_deadline};
use crate::subscription::driver::input::WebSocketOperationIngress;
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::worker::websocket::{
    WebSocketTerminalMessage, WebSocketValidationOutcome, WebSocketValidationWorker,
};
use probe::{BlockingGate, Decoded, IngressProbe, Source, SourceMode};

#[test]
fn source_and_decoded_hooks_run_once_before_join_without_creating_context() {
    let app = State::new(Vec::new());
    let probe = Arc::new(IngressProbe::default());
    let document: Arc<str> = Arc::from(echo_request().query);
    let lifecycle = lifecycle(
        Arc::clone(&probe),
        SourceMode::Document(document),
        None,
        None,
    );
    let registered = registered(Arc::clone(&app), lifecycle, 16 * 1024);
    let worker = ingress_worker(
        &registered,
        RequestEnvelope::document(echo_request()),
        "source-and-decoded",
        &super::support::NEVER_DEADLINES,
    );

    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, worker) else {
        panic!("resolved WebSocket query did not prepare")
    };
    assert_eq!(
        prepared.kind(),
        chigiri_runtime::__private::WebSocketOperationKind::Query
    );
    assert_eq!(probe.source_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.decoded_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *probe.decoded_policy.lock().unwrap(),
        [(RequestMethod::Post, ResponseMediaType::GraphQlResponseJson)]
    );
    assert_eq!(count(&app.counts.scalar_calls), 1);
    assert_eq!(count(&app.counts.factory_calls), 0);
    drop(prepared);
    registered.drain();
}

#[test]
fn final_resolved_document_limit_precedes_decoded_hook_and_validation() {
    let app = State::new(Vec::new());
    let probe = Arc::new(IngressProbe::default());
    let lifecycle = lifecycle(
        Arc::clone(&probe),
        SourceMode::Document(Arc::from("x".repeat(8 * 1024))),
        None,
        None,
    );
    let registered = registered(Arc::clone(&app), lifecycle, 1024);
    let worker = ingress_worker(
        &registered,
        RequestEnvelope::document(GraphQlRequest::new("query { ping }")),
        "resolved-limit",
        &super::support::NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Terminal(mut terminal) = run(&registered.runtime, worker)
    else {
        panic!("oversized resolved document did not become a request error")
    };
    let WebSocketTerminalMessage::Error(response_value) = terminal.take_message().unwrap() else {
        panic!("oversized resolved document did not select an error terminal")
    };
    assert_eq!(error_code(&response_value), Some("BAD_USER_INPUT"));
    drop(terminal);
    assert_eq!(probe.source_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.decoded_calls.load(Ordering::SeqCst), 0);
    assert_eq!(count(&app.counts.scalar_calls), 0);
    assert_eq!(count(&app.counts.factory_calls), 0);
    registered.drain();
}

#[test]
fn source_rejection_is_a_single_observed_graphql_request_error() {
    let app = State::new(Vec::new());
    let probe = Arc::new(IngressProbe::default());
    let lifecycle = lifecycle(Arc::clone(&probe), SourceMode::Reject, None, None);
    let registered = registered(Arc::clone(&app), lifecycle, 16 * 1024);
    let worker = ingress_worker(
        &registered,
        RequestEnvelope::document(echo_request()),
        "source-rejection",
        &super::support::NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Terminal(mut terminal) = run(&registered.runtime, worker)
    else {
        panic!("source rejection did not become a request error")
    };
    let WebSocketTerminalMessage::Error(response_value) = terminal.take_message().unwrap() else {
        panic!("source rejection did not select an error terminal")
    };
    assert_eq!(error_code(&response_value), Some("SOURCE_DENIED"));
    drop(terminal);
    assert_eq!(probe.source_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.decoded_calls.load(Ordering::SeqCst), 0);
    assert_eq!(count(&app.counts.scalar_calls), 0);
    assert_eq!(count(&app.counts.factory_calls), 0);
    registered.drain();
}

#[test]
fn original_setup_deadline_is_external_to_source_and_decoded_callbacks() {
    for block_source in [true, false] {
        let app = State::new(Vec::new());
        let probe = Arc::new(IngressProbe::default());
        let gate = Arc::new(BlockingGate::default());
        let document: Arc<str> = Arc::from(echo_request().query);
        let lifecycle = lifecycle(
            Arc::clone(&probe),
            SourceMode::Document(document),
            block_source.then(|| Arc::clone(&gate)),
            (!block_source).then(|| Arc::clone(&gate)),
        );
        let registered = registered(Arc::clone(&app), lifecycle, 16 * 1024);
        let deadlines = Box::leak(Box::new(TokioDeadlines::default()));
        let _release_on_drop = gate.release_on_drop();
        let worker = ingress_worker(
            &registered,
            RequestEnvelope::document(echo_request()),
            if block_source {
                "blocked-source"
            } else {
                "blocked-decoded"
            },
            deadlines,
        );
        gate.wait_entered();
        // Keep the callback blocked until the external controller actually selects timeout.
        let outcome = run(&registered.runtime, worker);
        gate.release();
        assert!(matches!(outcome, WebSocketValidationOutcome::Terminal(_)));
        drop(outcome);
        assert_eq!(
            deadlines
                .starts()
                .into_iter()
                .filter(|duration| *duration == super::super::deadlines::SETUP_TIMEOUT)
                .count(),
            1
        );
        assert_eq!(probe.source_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            probe.decoded_calls.load(Ordering::SeqCst),
            usize::from(!block_source)
        );
        assert_eq!(count(&app.counts.scalar_calls), 0);
        assert_eq!(count(&app.counts.factory_calls), 0);
        registered.drain();
    }
}

fn lifecycle(
    probe: Arc<IngressProbe>,
    mode: SourceMode,
    source_gate: Option<Arc<BlockingGate>>,
    decoded_gate: Option<Arc<BlockingGate>>,
) -> HttpLifecycle {
    HttpLifecycle::new()
        .operation_source(Source {
            probe: Arc::clone(&probe),
            mode,
            gate: source_gate,
        })
        .decoded_request(Decoded {
            probe,
            gate: decoded_gate,
        })
}

fn registered(app: Arc<State>, lifecycle: HttpLifecycle, metadata_limit: usize) -> Registered {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let endpoint = harness::endpoint_with_lifecycle(app, metadata_limit, lifecycle);
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let connection = endpoint.subscriptions.resources.open_connection().unwrap();
    Registered {
        runtime,
        endpoint,
        guard,
        connection,
    }
}

fn ingress_worker(
    registered: &Registered,
    envelope: RequestEnvelope,
    id: &'static str,
    deadlines: &'static dyn chigiri_http::DeadlineFactory,
) -> WebSocketValidationWorker<super::super::state::AppContext> {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"original-secret")];
    let transport = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    let telemetry = registered.endpoint.schema.request_telemetry(None);
    let ingress = WebSocketOperationIngress::capture(
        Arc::clone(&registered.endpoint.subscription_execution),
        registered.endpoint.lifecycle.clone(),
        transport,
        envelope,
        telemetry,
        None,
    )
    .unwrap();
    let operation = registered.begin(Arc::from(id));
    let deadline = setup_deadline(deadlines);
    let _entered = registered.runtime.enter();
    let failure = SubscriptionFailureReporter::new(
        registered.endpoint.subscription_execution.schema.clone(),
        registered.endpoint.schema.request_telemetry(None),
        "subscription-websocket-worker-test",
    );
    WebSocketValidationWorker::new(ingress, operation, deadline, failure)
        .unwrap_or_else(|_| panic!("WebSocket ingress validation worker did not start"))
}

fn error_code(response: &chigiri_runtime::GraphQlResponse) -> Option<&str> {
    response
        .errors
        .first()
        .and_then(|error| error.extensions.get("code"))
        .and_then(Value::as_str)
}
