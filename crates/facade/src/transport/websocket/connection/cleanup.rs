//! Generation-safe connection termination and bounded physical cleanup.

pub(super) mod disposal;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::authorization::AuthorizationResult;
use super::close::WebSocketConnectionClose;
use crate::subscription::bridge_resources::{
    BridgeCleanupCoordinator, BridgeCleanupOutcome, BridgeConnection, DetachedCleanup,
};
use crate::subscription::worker::{ConcreteWorker, RetiredWorkerOwner, WorkerRetainedState};

/// Application-polled physical cleanup after one logical connection close.
#[must_use = "connection cleanup must be polled or dropped into quarantine"]
#[doc(hidden)]
pub struct WebSocketConnectionCleanup {
    close: WebSocketConnectionClose,
    coordinator: Option<BridgeCleanupCoordinator<'static>>,
}

impl WebSocketConnectionCleanup {
    /// Returns the fixed safe close to send before draining physical work.
    pub const fn close(&self) -> WebSocketConnectionClose {
        self.close
    }

    fn coordinated(
        close: WebSocketConnectionClose,
        coordinator: BridgeCleanupCoordinator<'static>,
    ) -> Self {
        Self {
            close,
            coordinator: Some(coordinator),
        }
    }

    fn failed_closed(close: WebSocketConnectionClose) -> Self {
        Self {
            close,
            coordinator: None,
        }
    }
}

impl Future for WebSocketConnectionCleanup {
    type Output = BridgeCleanupOutcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(coordinator) = &mut self.coordinator else {
            return Poll::Pending;
        };
        Pin::new(coordinator).poll(context)
    }
}

pub(super) fn framework(
    connection: BridgeConnection,
    retained: disposal::ConnectionDisposal,
    close: WebSocketConnectionClose,
    deadlines: &'static dyn DeadlineFactory,
) -> WebSocketConnectionCleanup {
    // Dispose this closed metadata before claiming completion, outside all
    // locks, with the connection owner still live. Even cooperative timer or
    // waker disposal cannot release that physical owner before returning.
    drop(retained);
    let cleanup = DetachedCleanup::completed();
    match connection.revoke() {
        Ok(terminal) => {
            WebSocketConnectionCleanup::coordinated(close, terminal.coordinate(cleanup, deadlines))
        }
        Err(_) => {
            std::mem::forget(cleanup);
            WebSocketConnectionCleanup::failed_closed(close)
        }
    }
}

pub(super) fn worker(
    connection: BridgeConnection,
    worker: ConcreteWorker<AuthorizationResult>,
    deadline: BoxFuture<'static, ()>,
    close: WebSocketConnectionClose,
    deadlines: &'static dyn DeadlineFactory,
) -> WebSocketConnectionCleanup {
    let retained = WorkerRetainedState::connection_init(deadline);
    match connection.revoke() {
        Ok(terminal) => {
            worker.abort();
            let cleanup = DetachedCleanup::worker(worker.erase(), retained);
            WebSocketConnectionCleanup::coordinated(close, terminal.coordinate(cleanup, deadlines))
        }
        Err(_) => {
            worker.abort();
            worker.erase().retire(RetiredWorkerOwner::orphan(retained));
            WebSocketConnectionCleanup::failed_closed(close)
        }
    }
}
