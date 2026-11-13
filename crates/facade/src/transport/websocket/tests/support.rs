use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    AdmissionDecision, AdmissionError, ApplicationMetadata, BoxFuture, ConnectionInitAuthorizer,
    ContextFactory, DeadlineFactory, OperationSourceResolver, PrepareError, PreparedContext,
    Rejection, RequestContext, RequestEnvelope, RequestHeader, ResolvedOperationSource,
    SourceResolutionError, TransportAdmission, TransportRequest, WebSocketHttpVersion,
};
use chigiri_runtime::{
    ErrorEvent, ErrorObserver, GraphQlError, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult,
};

use crate::subscription::request::test_support::schema;
use crate::{Endpoint, SubscriptionLimits, SubscriptionProtocolSettings};

use super::super::WebSocketUpgradeIngress;

pub(super) const ADMISSION_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const UPGRADE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
pub(super) enum AdmissionMode {
    Continue,
    Reject,
    Internal,
    Pending,
}

#[derive(Default)]
pub(super) struct Signals {
    pub(super) admission: AtomicUsize,
    pub(super) source: AtomicUsize,
    pub(super) context: AtomicUsize,
    pub(super) internal_errors: AtomicUsize,
    pub(super) lifecycle: Mutex<Vec<(LifecyclePhase, LifecycleResult)>>,
}

struct Admission {
    mode: AdmissionMode,
    signals: Arc<Signals>,
}

impl TransportAdmission for Admission {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        assert!(request.request_telemetry().is_some());
        assert!(request.deadline_factory().is_some());
        self.signals.admission.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            AdmissionMode::Continue => Box::pin(async { Ok(AdmissionDecision::Continue) }),
            AdmissionMode::Reject => Box::pin(async {
                Ok(AdmissionDecision::Reject(
                    Rejection::new(429, GraphQlError::new("Not now").code("NOT_NOW")).unwrap(),
                ))
            }),
            AdmissionMode::Internal => {
                Box::pin(async { Err(AdmissionError::new(TestError("admission failed"))) })
            }
            AdmissionMode::Pending => Box::pin(std::future::pending()),
        }
    }
}

struct Source(Arc<Signals>);

impl OperationSourceResolver for Source {
    fn resolve<'a>(
        &'a self,
        _: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        self.0.source.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { panic!("WebSocket opening admission must not resolve operations") })
    }
}

struct Factory(Arc<Signals>);

impl ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        self.0.context.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { panic!("WebSocket opening admission must not create context") })
    }
}

struct Observer(Arc<Signals>);

impl LifecycleObserver for Observer {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .lifecycle
            .lock()
            .unwrap()
            .push((event.phase(), event.result()));
    }
}

struct Errors(Arc<Signals>);

impl ErrorObserver for Errors {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.internal_errors.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct TestError(&'static str);

impl std::fmt::Display for TestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for TestError {}

pub(super) fn endpoint(
    mode: AdmissionMode,
    connections: usize,
    metadata_limit: usize,
) -> (Endpoint<()>, Arc<Signals>) {
    endpoint_with_authorizer(
        mode,
        connections,
        metadata_limit,
        chigiri_http::AcceptConnectionInit,
    )
}

pub(super) fn endpoint_with_authorizer(
    mode: AdmissionMode,
    connections: usize,
    metadata_limit: usize,
    authorizer: impl ConnectionInitAuthorizer,
) -> (Endpoint<()>, Arc<Signals>) {
    let signals = Arc::new(Signals::default());
    let limits = SubscriptionLimits {
        max_retained_metadata_bytes: metadata_limit,
        ..SubscriptionLimits::default()
    };
    let protocol = SubscriptionProtocolSettings {
        max_websocket_connections: connections,
        ..SubscriptionProtocolSettings::default()
    };
    let endpoint = Endpoint::builder(schema())
        .context_factory(Factory(Arc::clone(&signals)))
        .operation_source(Source(Arc::clone(&signals)))
        .connection_init_authorizer(authorizer)
        .subscription_limits(limits)
        .subscription_protocol(protocol)
        .inject(|components| {
            components
                .http()
                .push_transport_admission(Admission {
                    mode,
                    signals: Arc::clone(&signals),
                })
                .transport_admission_timeout(ADMISSION_TIMEOUT);
            components
                .observability()
                .lifecycle_observer(Observer(Arc::clone(&signals)))
                .error_observer(Errors(Arc::clone(&signals)));
        })
        .build()
        .unwrap();
    signals.lifecycle.lock().unwrap().clear();
    (endpoint, signals)
}

pub(super) fn valid_headers(with_csrf: bool) -> Vec<RequestHeader<'static>> {
    let mut headers = vec![
        RequestHeader::new("host", b"example.test"),
        RequestHeader::new("connection", b"keep-alive, Upgrade"),
        RequestHeader::new("upgrade", b"websocket"),
        RequestHeader::new("sec-websocket-version", b"13"),
        RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
        RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
    ];
    if with_csrf {
        headers.push(RequestHeader::new("x-chigiri-preflight", b"1"));
    }
    headers
}

pub(super) fn invoke<'deadline>(
    endpoint: &Endpoint<()>,
    deadlines: &'deadline ManualDeadlines,
    method: &str,
    headers: &[RequestHeader<'_>],
    metadata: &ApplicationMetadata,
) -> WebSocketUpgradeIngress<'deadline> {
    let request = TransportRequest::new(method, "/graphql", None, headers, None, metadata);
    ready(endpoint.__admit_websocket_upgrade(request, WebSocketHttpVersion::Http11, deadlines))
}

#[derive(Clone, Default)]
pub(super) struct ManualDeadlines {
    requested: Arc<Mutex<Vec<Duration>>>,
    expired: Arc<Mutex<BTreeSet<Duration>>>,
}

impl ManualDeadlines {
    pub(super) fn expire(&self, duration: Duration) {
        self.expired.lock().unwrap().insert(duration);
    }

    pub(super) fn requested(&self) -> Vec<Duration> {
        self.requested.lock().unwrap().clone()
    }
}

impl DeadlineFactory for ManualDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.requested.lock().unwrap().push(duration);
        let expired = Arc::clone(&self.expired);
        Box::pin(std::future::poll_fn(move |_| {
            if expired.lock().unwrap().contains(&duration) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

pub(super) fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future did not complete in one deterministic poll"),
    }
}

pub(super) fn finite(result: WebSocketUpgradeIngress<'_>) -> chigiri_http::ResponseParts {
    match result {
        WebSocketUpgradeIngress::Finite(response) => response,
        WebSocketUpgradeIngress::Admitted(seed) => {
            seed.cancel();
            panic!("expected finite WebSocket upgrade result")
        }
    }
}

pub(super) fn assert_no_execution(signals: &Signals) {
    assert_eq!(signals.source.load(Ordering::SeqCst), 0);
    assert_eq!(signals.context.load(Ordering::SeqCst), 0);
}
