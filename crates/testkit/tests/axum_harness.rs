use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use chigiri::{Endpoint, EndpointBuilder};
use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    ResponseHeaders, ResponseHook, ResponseHookContext,
};
use chigiri_testkit::{AdapterConformanceError, AdapterScenario, LifecycleHarness};
use futures_util::Stream;
use http_body_util::BodyExt as _;

use crate::lifecycle_support::{
    PendingDecoded, PendingTransport, PhaseLog, RecordingDecoded, RecordingSource,
    RecordingTransport, TestContext, endpoint_builder, rejection,
};

pub struct AxumHarness;

impl LifecycleHarness for AxumHarness {
    fn run(
        &self,
        scenario: AdapterScenario,
    ) -> chigiri_http::BoxFuture<'_, Result<(), AdapterConformanceError>> {
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

pub(crate) struct Factory {
    pub(crate) log: PhaseLog,
    pub(crate) reject: bool,
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
            if forbidden {
                headers
                    .append("content-type", "text/plain")
                    .map_err(HookError::new)?;
            } else {
                headers
                    .append("x-lifecycle", "ok")
                    .map_err(HookError::new)?;
            }
            Ok(headers)
        })
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
    let response = call(endpoint, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::OK);
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
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(RecordingTransport {
                    log: PhaseLog::default(),
                    reject: true,
                });
        })
        .context_factory(Factory {
            log: PhaseLog::default(),
            reject: false,
        })
        .build()
        .unwrap();
    let body = Body::from_stream(PollCountingStream(Arc::clone(&polls)));
    let response = call(endpoint, post(body)).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}

async fn source_reject() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), true, false, None).build().unwrap();
    assert_eq!(
        call(endpoint, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(!log.snapshot().contains(&"context"));
}

async fn context_reject() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), false, true, None).build().unwrap();
    assert_eq!(
        call(endpoint, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(!log.snapshot().contains(&"request"));
}

async fn operation_reject() {
    let log = PhaseLog::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint_builder(log.clone(), Arc::clone(&calls), true)
        .context_factory(Factory { log, reject: false })
        .build()
        .unwrap();
    assert_eq!(
        call(endpoint, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

async fn timeout_drop() {
    let transport_drop = Arc::new(AtomicBool::new(false));
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(PendingTransport(Arc::clone(&transport_drop)))
                .transport_admission_timeout(Duration::from_millis(5));
        })
        .context_factory(Factory {
            log: PhaseLog::default(),
            reject: false,
        })
        .build()
        .unwrap();
    assert_eq!(
        call(endpoint, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::GATEWAY_TIMEOUT
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
    let response = call(endpoint, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers()["content-type"],
        "application/graphql-response+json; charset=utf-8"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("PERSISTED_OPERATION_UNAVAILABLE")
    );
    assert!(dropped.load(Ordering::SeqCst));

    let decoded_drop = Arc::new(AtomicBool::new(false));
    let endpoint = endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            components
                .http()
                .push_decoded_hook(PendingDecoded(Arc::clone(&decoded_drop)))
                .decoded_request_timeout(Duration::from_millis(5));
        })
        .context_factory(Factory {
            log: PhaseLog::default(),
            reject: false,
        })
        .build()
        .unwrap();
    assert_eq!(
        call(endpoint, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert!(decoded_drop.load(Ordering::SeqCst));
}

async fn csrf() {
    let log = PhaseLog::default();
    let endpoint = endpoint_builder(log.clone(), Arc::new(AtomicUsize::new(0)), false)
        .context_factory(Factory { log, reject: false })
        .build()
        .unwrap();
    assert_eq!(
        call(endpoint.clone(), get(false)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(call(endpoint, get(true)).await.status(), StatusCode::OK);
}

async fn header_authority() {
    let log = PhaseLog::default();
    let endpoint = configured(log.clone(), false, false, None)
        .response_hook(RecordingResponse {
            log,
            forbidden: true,
        })
        .build()
        .unwrap();
    let response = call(endpoint, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()["content-type"],
        "application/graphql-response+json; charset=utf-8"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("INTERNAL_SERVER_ERROR")
    );
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

async fn call(endpoint: Endpoint<TestContext>, request: Request) -> axum::response::Response {
    chigiri_axum::handler(endpoint)(request).await
}

fn post(body: Body) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(body)
        .unwrap()
}

fn get(preflight: bool) -> Request {
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri("/graphql?query=%7Bhello%7D")
        .header("accept", "application/graphql-response+json");
    if preflight {
        builder = builder.header("x-chigiri-preflight", "1");
    }
    builder.body(Body::empty()).unwrap()
}

const fn query_body() -> &'static str {
    r#"{"query":"{ hello }"}"#
}

struct PollCountingStream(Arc<AtomicUsize>);

impl Stream for PollCountingStream {
    type Item = Result<bytes::Bytes, io::Error>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Poll::Pending
    }
}
