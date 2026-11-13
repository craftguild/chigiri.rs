use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::EndpointBuilder;
use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    ResponseHeaders, ResponseHook, ResponseHookContext,
};
use chigiri_testkit::{AdapterConformanceError, AdapterScenario, LifecycleHarness};

use crate::hyper_wire::{
    JSON_HEADERS, get_request, post_request, request, request_without_request_body, spawn,
};
use crate::lifecycle_support::{
    PendingDecoded, PendingTransport, PhaseLog, RecordingDecoded, RecordingSource,
    RecordingTransport, TestContext, endpoint_builder, rejection,
};

pub struct HyperHarness;

impl LifecycleHarness for HyperHarness {
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

struct ForbiddenResponse;

impl ResponseHook<()> for ForbiddenResponse {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        Box::pin(async {
            let mut headers = ResponseHeaders::new();
            headers
                .append("content-type", "text/plain")
                .map_err(HookError::new)?;
            Ok(headers)
        })
    }
}

async fn phase_order() {
    let log = PhaseLog::default();
    let endpoint = recording(log.clone(), false, None).build().unwrap();
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 200);
    assert_eq!(
        log.snapshot(),
        [
            "transport",
            "source",
            "decoded",
            "context",
            "request",
            "operation",
            "resolver"
        ]
    );
    running.stop().await;
}

async fn transport_reject() {
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
    let running = spawn(endpoint).await;
    let headers_only = format!(
        "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Length: 999\r\nConnection: close\r\n{JSON_HEADERS}\r\n"
    );
    let response = request_without_request_body(running.address, headers_only).await;
    assert_eq!(response.status, 429);
    running.stop().await;
}

async fn source_reject() {
    let log = PhaseLog::default();
    let endpoint = recording(log.clone(), true, None).build().unwrap();
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 403);
    assert!(!log.snapshot().contains(&"context"));
    running.stop().await;
}

async fn context_reject() {
    let endpoint = basic(PhaseLog::default(), true, false).build().unwrap();
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 403);
    running.stop().await;
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
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 422);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    running.stop().await;
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
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 504);
    assert!(transport_drop.load(Ordering::SeqCst));
    running.stop().await;

    let dropped = Arc::new(AtomicBool::new(false));
    let endpoint = recording(PhaseLog::default(), false, Some(Arc::clone(&dropped)))
        .inject(|components| {
            components
                .http()
                .source_resolution_timeout(Duration::from_millis(5));
        })
        .build()
        .unwrap();
    let running = spawn(endpoint).await;
    let response = request(running.address, post_request()).await;
    assert_eq!(response.status, 503);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/graphql-response+json; charset=utf-8")
    );
    assert!(dropped.load(Ordering::SeqCst));
    running.stop().await;

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
    let running = spawn(endpoint).await;
    assert_eq!(request(running.address, post_request()).await.status, 504);
    assert!(decoded_drop.load(Ordering::SeqCst));
    running.stop().await;
}

async fn csrf() {
    let running = spawn(basic(PhaseLog::default(), false, false).build().unwrap()).await;
    assert_eq!(
        request(running.address, get_request(false)).await.status,
        403
    );
    assert_eq!(
        request(running.address, get_request(true)).await.status,
        200
    );
    running.stop().await;
}

async fn header_authority() {
    let endpoint = basic(PhaseLog::default(), false, false)
        .response_hook(ForbiddenResponse)
        .build()
        .unwrap();
    let running = spawn(endpoint).await;
    let response = request(running.address, post_request()).await;
    assert_eq!(response.status, 500);
    assert_eq!(
        response.content_type.as_deref(),
        Some("application/graphql-response+json; charset=utf-8")
    );
    running.stop().await;
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

fn recording(
    log: PhaseLog,
    source_reject: bool,
    pending_drop: Option<Arc<AtomicBool>>,
) -> EndpointBuilder<TestContext, (), Factory> {
    basic(log.clone(), false, false).inject(|components| {
        components
            .http()
            .push_transport_admission(RecordingTransport {
                log: log.clone(),
                reject: false,
            })
            .push_decoded_hook(RecordingDecoded(log.clone()));
        components.operation().source(RecordingSource {
            log,
            reject: source_reject,
            pending_drop,
        });
    })
}
