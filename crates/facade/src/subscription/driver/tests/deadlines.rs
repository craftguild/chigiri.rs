//! Deterministic phase and inherited deadline fixtures.

use std::future::pending;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

pub(super) const CONTEXT_TIMEOUT: Duration = Duration::from_millis(11);
pub(super) const EXECUTION_TIMEOUT: Duration = Duration::from_millis(12);
pub(super) const HOOK_TIMEOUT: Duration = Duration::from_millis(13);
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_millis(14);
pub(super) const SETUP_TIMEOUT: Duration = Duration::from_millis(15);
pub(super) const EVENT_TIMEOUT: Duration = Duration::from_millis(16);

pub(super) struct Deadlines(pub(super) Vec<Duration>);

impl DeadlineFactory for Deadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if self.0.contains(&duration) {
            Box::pin(async {})
        } else {
            Box::pin(pending())
        }
    }
}

pub(super) struct SecondPollDeadline(pub(super) Duration);

impl DeadlineFactory for SecondPollDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if duration != self.0 {
            return Box::pin(pending());
        }
        let mut polled = false;
        Box::pin(std::future::poll_fn(move |context: &mut Context<'_>| {
            if polled {
                Poll::Ready(())
            } else {
                polled = true;
                context.waker().wake_by_ref();
                Poll::Pending
            }
        }))
    }
}
