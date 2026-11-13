use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use chigiri_http::{
    ApplicationMetadata, BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext,
    ConnectionInitDecision, ConnectionInitError,
};

use super::super::super::{
    AdmittedWebSocketUpgrade, WebSocketConnectionHandoffResolution,
    WebSocketConnectionInitialization, WebSocketUpgradeIngress,
};
use super::super::support::{
    AdmissionMode, ManualDeadlines, endpoint_with_authorizer, invoke, valid_headers,
};
use crate::__streaming::SubscriptionRuntimeGuard;
use crate::Endpoint;

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap()
}

pub(super) fn admitted(
    endpoint: &Endpoint<()>,
    deadlines: &'static ManualDeadlines,
) -> AdmittedWebSocketUpgrade<'static> {
    match invoke(
        endpoint,
        deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    ) {
        WebSocketUpgradeIngress::Admitted(seed) => seed,
        WebSocketUpgradeIngress::Finite(response) => {
            panic!("expected admitted upgrade, got {}", response.status)
        }
    }
}

pub(super) fn initialization(
    endpoint: &Endpoint<()>,
    deadlines: &'static ManualDeadlines,
) -> WebSocketConnectionInitialization {
    let claimed = endpoint
        .__claim_websocket_upgrade(admitted(endpoint, deadlines))
        .unwrap();
    let handoff = endpoint
        .__begin_websocket_connection(claimed, deadlines)
        .unwrap();
    match ready(handoff) {
        WebSocketConnectionHandoffResolution::Initializing(initialization) => *initialization,
        WebSocketConnectionHandoffResolution::Closing(_) => {
            panic!("original upgrade deadline unexpectedly expired")
        }
    }
}

pub(super) fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
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

pub(super) fn finish_guard(runtime: &tokio::runtime::Runtime, guard: SubscriptionRuntimeGuard) {
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[derive(Clone, Copy)]
pub(super) enum Decision {
    Accept,
    Reject,
    Error,
    Panic,
}

pub(super) struct Authorizer {
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) decision: Decision,
}

impl ConnectionInitAuthorizer for Authorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.decision {
            Decision::Accept => Box::pin(async { Ok(ConnectionInitDecision::Accept) }),
            Decision::Reject => Box::pin(async { Ok(ConnectionInitDecision::Reject) }),
            Decision::Error => Box::pin(async {
                Err(ConnectionInitError::new(TestError(
                    "private authorization failure",
                )))
            }),
            Decision::Panic => Box::pin(async { panic!("private authorization panic") }),
        }
    }
}

pub(super) fn endpoint(
    authorizer: Authorizer,
    connections: usize,
    metadata_limit: usize,
) -> Endpoint<()> {
    endpoint_with_authorizer(
        AdmissionMode::Continue,
        connections,
        metadata_limit,
        authorizer,
    )
    .0
}

#[derive(Debug)]
struct TestError(&'static str);

impl std::fmt::Display for TestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for TestError {}
