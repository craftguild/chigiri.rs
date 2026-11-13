use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::{Endpoint, EndpointBuilder};
use chigiri_http::{
    ApplicationMetadata, BodyReadError, BoxFuture, BufferedBody, ContextFactory, DeadlineFactory,
    HookError, PrepareError, PreparedContext, RequestBody, RequestContext, RequestHeader,
    ResponseHeaders, ResponseHook, ResponseHookContext, TransportRequest,
};
use chigiri_testkit::{AdapterConformanceError, AdapterScenario, LifecycleHarness};

use crate::lifecycle_support::{
    PendingDecoded, PendingTransport, PhaseLog, RecordingDecoded, RecordingSource,
    RecordingTransport, TestContext, endpoint_builder, rejection,
};

pub struct NeutralHarness;

impl LifecycleHarness for NeutralHarness {
    fn run(&self, scenario: AdapterScenario) -> BoxFuture<'_, Result<(), AdapterConformanceError>> {
        Box::pin(async move {
            match scenario {
                AdapterScenario::PhaseOrder => phase_order().await,
                AdapterScenario::TransportRejectBeforeBody => transport_reject().await,
                AdapterScenario::SourceReject => source_reject().await,
                AdapterScenario::ContextReject => context_reject().await,
                AdapterScenario::OperationReject => operation_reject().await,
                AdapterScenario::PhaseTimeoutDrop => timeout_drop().await,
                AdapterScenario::CsrfSimpleRequest => csrf().await,
                AdapterScenario::ResponseHeaderAuthority => header_authority().await,
            }
            Ok(())
        })
    }
}

struct Factory {
    log: PhaseLog,
    reject: bool,
}

impl ContextFactory<TestContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        self.log.push("context");
        let log = self.log.clone();
        let reject = self.reject;
        Box::pin(async move {
            if reject {
                Err(PrepareError::Reject(rejection(
                    403,
                    "Context rejected",
                    "CONTEXT_REJECTED",
                )))
            } else {
                Ok(PreparedContext::execution_only(TestContext { log }))
            }
        })
    }
}

struct RecordingResponse {
    log: PhaseLog,
    forbidden: bool,
}

impl ResponseHook<()> for RecordingResponse {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.log.push("response");
        let forbidden = self.forbidden;
        Box::pin(async move {
            let mut headers = ResponseHeaders::new();
            let (name, value) = if forbidden {
                ("content-type", "text/plain")
            } else {
                ("x-lifecycle", "ok")
            };
            headers.append(name, value).map_err(HookError::new)?;
            Ok(headers)
        })
    }
}

struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

async fn phase_order() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), false, false, None)
        .response_hook(RecordingResponse {
            log: log.clone(),
            forbidden: false,
        })
        .build()
        .unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        200
    );
    assert_eq!(
        log.snapshot(),
        [
            "transport",
            "source",
            "decoded",
            "context",
            "request",
            "operation",
            "resolver",
            "response"
        ]
    );
}

async fn transport_reject() {
    let polls = Arc::new(AtomicUsize::new(0));
    let endpoint = basic(PhaseLog::default(), false, false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(RecordingTransport {
                    log: PhaseLog::default(),
                    reject: true,
                });
        })
        .build()
        .unwrap();
    let response = post(endpoint, CountingBody(Arc::clone(&polls))).await;
    assert_eq!(response.status, 429);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}

async fn source_reject() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), true, false, None).build().unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        403
    );
    assert!(!log.snapshot().contains(&"context"));
}

async fn context_reject() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), false, true, None).build().unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        403
    );
    assert!(!log.snapshot().contains(&"request"));
}

async fn operation_reject() {
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::clone(&calls), true)
        .context_factory(Factory {
            log: PhaseLog::default(),
            reject: false,
        })
        .build()
        .unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        422
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

async fn timeout_drop() {
    let transport_drop = Arc::new(AtomicBool::new(false));
    let endpoint = basic(PhaseLog::default(), false, false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(PendingTransport(Arc::clone(&transport_drop)))
                .transport_admission_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        504
    );
    assert!(transport_drop.load(Ordering::SeqCst));

    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = configured(
        PhaseLog::default(),
        false,
        false,
        Some(Arc::clone(&dropped)),
    )
    .inject(|components| {
        components
            .http()
            .source_resolution_timeout(Duration::from_millis(5));
    })
    .build()
    .unwrap();
    let response = post(endpoint, BufferedBody::new(query_body())).await;
    assert_eq!(response.status, 503);
    assert_eq!(
        response.headers[0].value,
        "application/graphql-response+json; charset=utf-8"
    );
    assert!(
        std::str::from_utf8(&response.body)
            .unwrap()
            .contains("PERSISTED_OPERATION_UNAVAILABLE")
    );
    assert!(dropped.load(Ordering::SeqCst));

    let decoded_drop = Arc::new(AtomicBool::new(false));
    let endpoint = basic(PhaseLog::default(), false, false)
        .inject(|components| {
            components
                .http()
                .push_decoded_hook(PendingDecoded(Arc::clone(&decoded_drop)))
                .decoded_request_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    assert_eq!(
        post(endpoint, BufferedBody::new(query_body())).await.status,
        504
    );
    assert!(decoded_drop.load(Ordering::SeqCst));
}

async fn csrf() {
    let endpoint = basic(PhaseLog::default(), false, false).build().unwrap();
    assert_eq!(get(endpoint.clone(), false).await.status, 403);
    assert_eq!(get(endpoint, true).await.status, 200);
}

async fn header_authority() {
    let endpoint = basic(PhaseLog::default(), false, false)
        .response_hook(RecordingResponse {
            log: PhaseLog::default(),
            forbidden: true,
        })
        .build()
        .unwrap();
    let response = post(endpoint, BufferedBody::new(query_body())).await;
    assert_eq!(response.status, 500);
    assert_eq!(response.headers[0].name, "content-type");
}

fn basic(
    log: PhaseLog,
    reject_context: bool,
    reject_operation: bool,
) -> EndpointBuilder<TestContext, (), Factory> {
    endpoint_builder(log.clone(), Arc::new(AtomicUsize::new(0)), reject_operation).context_factory(
        Factory {
            log,
            reject: reject_context,
        },
    )
}

fn configured(
    log: PhaseLog,
    source_reject: bool,
    context_reject: bool,
    pending_drop: Option<Arc<AtomicBool>>,
) -> EndpointBuilder<TestContext, (), Factory> {
    endpoint_builder(log.clone(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(RecordingTransport {
                    log: log.clone(),
                    reject: false,
                })
                .push_decoded_hook(RecordingDecoded(log.clone()));
            components.operation().source(RecordingSource {
                log: log.clone(),
                reject: source_reject,
                pending_drop,
            });
        })
        .context_factory(Factory {
            log,
            reject: context_reject,
        })
}

async fn post<B: RequestBody>(
    endpoint: Endpoint<TestContext>,
    body: B,
) -> chigiri_http::ResponseParts {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    handle(endpoint, "POST", "/graphql", None, &headers, body).await
}

async fn get(endpoint: Endpoint<TestContext>, preflight: bool) -> chigiri_http::ResponseParts {
    let header = RequestHeader::new("x-chigiri-preflight", b"1");
    let headers = if preflight {
        std::slice::from_ref(&header)
    } else {
        &[]
    };
    handle(
        endpoint,
        "GET",
        "/graphql",
        Some("query=%7Bhello%7D"),
        headers,
        BufferedBody::empty(),
    )
    .await
}

async fn handle<B: RequestBody>(
    endpoint: Endpoint<TestContext>,
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: &[RequestHeader<'_>],
    body: B,
) -> chigiri_http::ResponseParts {
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new(method, path, query, headers, None, &metadata);
    endpoint.handle(request, body, &TokioDeadline).await
}

fn query_body() -> Vec<u8> {
    br#"{"query":"{ hello }"}"#.to_vec()
}

struct CountingBody(Arc<AtomicUsize>);

impl RequestBody for CountingBody {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
}
