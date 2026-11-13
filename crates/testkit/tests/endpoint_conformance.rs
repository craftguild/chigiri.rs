#[path = "endpoint_support.rs"]
mod endpoint_support;
#[path = "lifecycle_support.rs"]
pub mod lifecycle_support;
#[path = "endpoint_conformance/output_diagnostics.rs"]
mod output_diagnostics;
#[path = "endpoint_conformance/response_completion.rs"]
mod response_completion;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri::{
    Endpoint, EndpointBuilder, GraphQlResponse, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult, Value,
};
use chigiri_http::{
    ApplicationMetadata, BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext,
    RequestBody, RequestContext, RequestHeader, ResponseMediaType, TransportRequest,
    encode_response,
};

use endpoint_support::{
    ChunkBody, CountingBody, Pending, PendingBody, PendingHook, PendingTransport, TokioDeadline,
};
use lifecycle_support::{
    PhaseLog, TestContext, endpoint_builder, recording_plugin, schema_builder_with_pending,
};

enum FactoryMode {
    Ready,
    Internal,
    Pending(Arc<AtomicBool>),
}

struct Factory(FactoryMode);

#[derive(Clone, Default)]
struct TerminalRecorder(Arc<Mutex<Vec<(LifecyclePhase, LifecycleResult)>>>);

impl LifecycleObserver for TerminalRecorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0.lock().unwrap().push((event.phase(), event.result()));
    }
}

impl ContextFactory<TestContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        match &self.0 {
            FactoryMode::Ready => Box::pin(async {
                Ok(PreparedContext::execution_only(TestContext {
                    log: PhaseLog::default(),
                }))
            }),
            FactoryMode::Internal => Box::pin(async {
                Err(PrepareError::Internal(HookError::new(
                    std::io::Error::other("private-context-source"),
                )))
            }),
            FactoryMode::Pending(dropped) => Box::pin(Pending::new(Arc::clone(dropped))),
        }
    }
}

#[tokio::test]
async fn handles_query_mutation_get_and_field_error_responses() {
    let endpoint = ready_endpoint();
    let query = post(endpoint.clone(), r#"{"query":"{ hello }"}"#).await;
    assert_eq!(query.status, 200);
    assert_eq!(json(&query)["data"]["hello"], "ok");

    let mutation = post(endpoint.clone(), r#"{"query":"mutation { touch }"}"#).await;
    assert_eq!(mutation.status, 200);
    assert_eq!(json(&mutation)["data"]["touch"], "ok");

    let get = get(endpoint.clone(), "query=%7Bhello%7D", dedicated_accept()).await;
    assert_eq!(get.status, 200);
    assert_eq!(json(&get)["data"]["hello"], "ok");

    let partial = post(endpoint, r#"{"query":"{ hello fail }"}"#).await;
    assert_eq!(partial.status, 200);
    assert_eq!(json(&partial)["data"]["hello"], "ok");
    assert_eq!(code(&partial).as_deref(), Some("SYNTHETIC_FAILURE"));
}

#[tokio::test]
async fn maps_media_shape_and_get_mutation_failures_to_status_codes() {
    let endpoint = ready_endpoint();
    let parse = post(endpoint.clone(), r#"{"query":"{"}"#).await;
    assert_eq!(parse.status, 400);
    assert_eq!(code(&parse).as_deref(), Some("GRAPHQL_PARSE_FAILED"));

    let validation = post(endpoint.clone(), r#"{"query":"{ missing }"}"#).await;
    assert_eq!(validation.status, 422);
    assert_eq!(
        code(&validation).as_deref(),
        Some("GRAPHQL_VALIDATION_FAILED")
    );

    let legacy = post_with_accept(
        endpoint.clone(),
        r#"{"query":"{ missing }"}"#,
        "application/json",
    )
    .await;
    assert_eq!(legacy.status, 200);

    let mutation = get(endpoint, "query=mutation%7Btouch%7D", dedicated_accept()).await;
    assert_eq!(mutation.status, 405);
    assert_eq!(header(&mutation, "allow"), Some("GET, POST"));
}

#[tokio::test]
async fn body_preflight_and_read_timeouts_bound_unfinished_streams() {
    let polls = Arc::new(AtomicUsize::new(0));
    let endpoint = ready_endpoint();
    let headers = [
        RequestHeader::new("content-type", b"text/plain"),
        RequestHeader::new("accept", dedicated_accept().as_bytes()),
    ];
    let response = handle(
        endpoint.clone(),
        "POST",
        None,
        &headers,
        CountingBody(Arc::clone(&polls)),
    )
    .await;
    assert_eq!(response.status, 415);
    assert_eq!(polls.load(Ordering::SeqCst), 0);

    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = builder(FactoryMode::Ready)
        .inject(|components| {
            components.http().body_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    let response = post_body(endpoint, PendingBody(Arc::clone(&dropped))).await;
    assert_eq!(response.status, 408);
    assert_eq!(code(&response).as_deref(), Some("REQUEST_BODY_TIMEOUT"));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn body_limit_timeout_and_context_failures_return_graphql_errors() {
    let polls = Arc::new(AtomicUsize::new(0));
    let endpoint = builder(FactoryMode::Ready).body_limit(8).build().unwrap();
    let response = post_body(
        endpoint,
        ChunkBody::new(vec![vec![b'x'; 9], b"unread".to_vec()], Arc::clone(&polls)),
    )
    .await;
    assert_eq!(response.status, 413);
    assert_eq!(polls.load(Ordering::SeqCst), 1);

    let internal = builder(FactoryMode::Internal).build().unwrap();
    let response = post(internal, r#"{"query":"{ hello }"}"#).await;
    assert_eq!(response.status, 500);
    assert_eq!(code(&response).as_deref(), Some("INTERNAL_SERVER_ERROR"));
    assert!(!String::from_utf8_lossy(&response.body).contains("private-context-source"));

    let context_drop = Arc::new(AtomicBool::new(false));
    let endpoint = builder(FactoryMode::Pending(Arc::clone(&context_drop)))
        .inject(|components| {
            components.http().context_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    assert_eq!(post(endpoint, r#"{"query":"{ hello }"}"#).await.status, 504);
    assert!(context_drop.load(Ordering::SeqCst));

    let execution_drop = Arc::new(AtomicBool::new(false));
    let lifecycle = TerminalRecorder::default();
    let execution_log = PhaseLog::default();
    let executable = schema_builder_with_pending(
        execution_log.clone(),
        Arc::new(AtomicUsize::new(0)),
        false,
        Some(Arc::clone(&execution_drop)),
    )
    .build()
    .unwrap();
    let endpoint = Endpoint::builder(executable)
        .push_plugin(recording_plugin(execution_log, false))
        .inject(|components| {
            components
                .http()
                .execution_timeout(Duration::from_millis(5));
            components
                .observability()
                .lifecycle_observer(lifecycle.clone());
        })
        .context_factory(Factory(FactoryMode::Ready))
        .build()
        .unwrap();
    assert_eq!(post(endpoint, r#"{"query":"{ hello }"}"#).await.status, 504);
    assert!(execution_drop.load(Ordering::SeqCst));
    assert!(
        lifecycle
            .0
            .lock()
            .unwrap()
            .contains(&(LifecyclePhase::RequestPlugin, LifecycleResult::Cancelled))
    );
}

#[tokio::test]
async fn response_byte_limit_returns_a_masked_internal_error() {
    let endpoint = builder(FactoryMode::Ready)
        .response_byte_limit(128)
        .build()
        .unwrap();
    let response = post(
        endpoint,
        r#"{"query":"{ response_field_01: hello response_field_02: hello response_field_03: hello response_field_04: hello response_field_05: hello response_field_06: hello response_field_07: hello response_field_08: hello }"}"#,
    )
    .await;

    assert_eq!(response.status, 500);
    assert_eq!(code(&response).as_deref(), Some("INTERNAL_SERVER_ERROR"));
    let body = String::from_utf8(response.body).unwrap();
    assert!(!body.contains("response_field_01"));
    assert!(body.len() <= 128);
}

#[tokio::test]
async fn response_hook_and_request_timeout_drop_the_target_future() {
    let hook_drop = Arc::new(AtomicBool::new(false));
    let endpoint = builder(FactoryMode::Ready)
        .response_hook(PendingHook(Arc::clone(&hook_drop)))
        .inject(|components| {
            components
                .http()
                .response_hook_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    assert_eq!(post(endpoint, r#"{"query":"{ hello }"}"#).await.status, 504);
    assert!(hook_drop.load(Ordering::SeqCst));

    let transport_drop = Arc::new(AtomicBool::new(false));
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(PendingTransport(Arc::clone(&transport_drop)))
                .transport_admission_timeout(Duration::from_secs(1))
                .overall_timeout(Duration::from_millis(5));
        })
        .context_factory(Factory(FactoryMode::Ready))
        .build()
        .unwrap();
    assert_eq!(post(endpoint, r#"{"query":"{ hello }"}"#).await.status, 504);
    assert!(transport_drop.load(Ordering::SeqCst));
}

#[tokio::test]
async fn in_flight_count_tracks_completion_and_cancellation() {
    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = builder(FactoryMode::Ready)
        .inject(|components| {
            components.http().body_timeout(Duration::from_secs(30));
        })
        .build()
        .unwrap();
    let observed = endpoint.clone();
    let task = tokio::spawn(post_body(endpoint, PendingBody(Arc::clone(&dropped))));
    tokio::task::yield_now().await;
    assert_eq!(observed.in_flight_requests(), 1);

    task.abort();
    let _ = task.await;
    assert_eq!(observed.in_flight_requests(), 0);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn returns_a_fallback_response_when_serialization_fails() {
    let response = GraphQlResponse::executed(Value::Float(f64::NAN));
    assert!(encode_response(ResponseMediaType::GraphQlResponseJson, &response).is_err());
    let fallback =
        chigiri_http::ResponseParts::serialization_failure(ResponseMediaType::GraphQlResponseJson);
    assert_eq!(fallback.status, 500);
    assert_eq!(
        header(&fallback, "content-type"),
        Some("application/graphql-response+json; charset=utf-8")
    );
}

fn ready_endpoint() -> Endpoint<TestContext> {
    builder(FactoryMode::Ready).build().unwrap()
}

fn builder(mode: FactoryMode) -> EndpointBuilder<TestContext, (), Factory> {
    endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .context_factory(Factory(mode))
}

async fn post(endpoint: Endpoint<TestContext>, body: &str) -> chigiri_http::ResponseParts {
    post_with_accept(endpoint, body, dedicated_accept()).await
}

async fn post_with_accept(
    endpoint: Endpoint<TestContext>,
    body: &str,
    accept: &str,
) -> chigiri_http::ResponseParts {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept.as_bytes()),
    ];
    handle(
        endpoint,
        "POST",
        None,
        &headers,
        chigiri_http::BufferedBody::new(body.as_bytes().to_vec()),
    )
    .await
}

async fn post_body<B: RequestBody>(
    endpoint: Endpoint<TestContext>,
    body: B,
) -> chigiri_http::ResponseParts {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", dedicated_accept().as_bytes()),
    ];
    handle(endpoint, "POST", None, &headers, body).await
}

async fn get(
    endpoint: Endpoint<TestContext>,
    query: &str,
    accept: &str,
) -> chigiri_http::ResponseParts {
    let headers = [
        RequestHeader::new("accept", accept.as_bytes()),
        RequestHeader::new("x-chigiri-preflight", b"1"),
    ];
    handle(
        endpoint,
        "GET",
        Some(query),
        &headers,
        chigiri_http::BufferedBody::empty(),
    )
    .await
}

async fn handle<B: RequestBody>(
    endpoint: Endpoint<TestContext>,
    method: &str,
    query: Option<&str>,
    headers: &[RequestHeader<'_>],
    body: B,
) -> chigiri_http::ResponseParts {
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new(method, "/graphql", query, headers, None, &metadata);
    endpoint.handle(request, body, &TokioDeadline).await
}

fn json(response: &chigiri_http::ResponseParts) -> serde_json::Value {
    serde_json::from_slice(&response.body).unwrap()
}

fn code(response: &chigiri_http::ResponseParts) -> Option<String> {
    json(response)["errors"][0]["extensions"]["code"]
        .as_str()
        .map(str::to_owned)
}

fn header<'a>(response: &'a chigiri_http::ResponseParts, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|header| header.name == name)
        .map(|header| header.value.as_str())
}

const fn dedicated_accept() -> &'static str {
    "application/graphql-response+json"
}
