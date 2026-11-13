//! Successful finite context output survives a failing raw future destructor.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, PrepareError, PreparedContext};

use super::super::support::Probe;

pub(super) struct ReadyDrop<'a> {
    future: BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>>,
    probe: Arc<Probe>,
    ready: bool,
}

impl<'a> ReadyDrop<'a> {
    pub(super) fn new(
        future: BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>>,
        probe: Arc<Probe>,
    ) -> Self {
        Self {
            future,
            probe,
            ready: false,
        }
    }
}

impl Future for ReadyDrop<'_> {
    type Output = Result<PreparedContext<(), ()>, PrepareError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self.future.as_mut().poll(cx);
        self.ready = result.is_ready();
        result
    }
}

impl Drop for ReadyDrop<'_> {
    fn drop(&mut self) {
        self.probe
            .context_future_drops
            .fetch_add(1, Ordering::SeqCst);
        assert!(
            self.ready,
            "a successful prepared value precedes the failed destructor"
        );
        panic!("private finite ready context future destruction failure");
    }
}
