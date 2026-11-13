//! Fixed-deadline cleanup coordination with Drop-safe ownership transfer.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::DeadlineFactory;

use super::cleanup::{
    DetachedCleanup, TerminalBridgeConnection, TerminalBridgeOperation, TerminalCleanupOwner,
};

const CLEANUP_WAIT: Duration = Duration::from_secs(5);

/// Observable result of the bounded active cleanup wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeCleanupOutcome {
    /// Cleanup completed and released its physical owners inside the wait.
    Completed,
    /// The fixed wait expired and transferred every owner to quarantine.
    Quarantined,
}

/// Move-only fixed-deadline cleanup future for one exact generation.
pub struct BridgeCleanupCoordinator<'a> {
    owner: Option<TerminalCleanupOwner>,
    cleanup: Option<DetachedCleanup>,
    deadline: Pin<Box<dyn Future<Output = ()> + Send + 'a>>,
}

impl<'a> BridgeCleanupCoordinator<'a> {
    fn new(
        owner: TerminalCleanupOwner,
        cleanup: DetachedCleanup,
        deadlines: &'a dyn DeadlineFactory,
    ) -> Self {
        Self {
            owner: Some(owner),
            cleanup: Some(cleanup),
            deadline: deadlines.sleep(CLEANUP_WAIT),
        }
    }

    fn quarantine(&mut self) {
        match (self.owner.take(), self.cleanup.take()) {
            (Some(owner), Some(cleanup)) => owner.quarantine(cleanup),
            (Some(owner), None) => owner.quarantine_completed(),
            (None, Some(cleanup)) => std::mem::forget(cleanup),
            (None, None) => {}
        }
    }

    fn complete(&mut self) {
        let Some(cleanup) = self.cleanup.take() else {
            self.quarantine();
            return;
        };
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(cleanup))) {
            if let Some(owner) = self.owner.take() {
                owner.quarantine_completed();
            }
            resume_unwind(payload);
        }
        if let Some(owner) = self.owner.take() {
            owner.finish();
        }
    }
}

impl Future for BridgeCleanupCoordinator<'_> {
    type Output = BridgeCleanupOutcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.deadline.as_mut().poll(context).is_ready() {
            self.quarantine();
            return Poll::Ready(BridgeCleanupOutcome::Quarantined);
        }
        let cleanup = self.cleanup.as_mut().expect("live cleanup coordinator");
        if Pin::new(cleanup).poll(context).is_ready() {
            self.complete();
            Poll::Ready(BridgeCleanupOutcome::Completed)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for BridgeCleanupCoordinator<'_> {
    fn drop(&mut self) {
        if self.owner.is_some() {
            self.quarantine();
        }
    }
}

impl TerminalBridgeOperation {
    /// Actively waits for operation cleanup with the fixed five-second bound.
    pub fn coordinate(
        self,
        cleanup: DetachedCleanup,
        deadlines: &dyn DeadlineFactory,
    ) -> BridgeCleanupCoordinator<'_> {
        BridgeCleanupCoordinator::new(self.into_cleanup_owner(), cleanup, deadlines)
    }
}

impl TerminalBridgeConnection {
    /// Actively waits for connection cleanup with the fixed five-second bound.
    pub fn coordinate(
        self,
        cleanup: DetachedCleanup,
        deadlines: &dyn DeadlineFactory,
    ) -> BridgeCleanupCoordinator<'_> {
        BridgeCleanupCoordinator::new(self.into_cleanup_owner(), cleanup, deadlines)
    }
}
