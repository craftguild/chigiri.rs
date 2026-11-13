//! Exact timer registration must precede a potentially blocking task-side call.

use std::task::{Context, Poll};

use super::{DeadlineState, TimerState};
use crate::subscription::worker::deadline::DeadlineRole;
use crate::subscription::worker::error::SubscriptionWorkerRuntimeError;

impl DeadlineState {
    pub(in crate::subscription::worker::deadline) fn poll_registered(
        &self,
        role: DeadlineRole,
        generation: u64,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        let mut replacement = Some(context.waker().clone());
        let mut inner = match self.lock() {
            Ok(inner) => inner,
            Err(error) => return Poll::Ready(Err(error)),
        };
        let unavailable = inner.closed || inner.corrupted;
        let slot = &mut inner.slots[role.index()];
        let (result, previous) = if unavailable
            || slot.generation != generation
            || matches!(slot.timer, TimerState::Empty)
        {
            (
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant)),
                None,
            )
        } else if slot.observed {
            (Poll::Ready(Ok(())), None)
        } else {
            (
                Poll::Pending,
                slot.proxy_waker
                    .replace(replacement.take().expect("proposed registration waker")),
            )
        };
        drop(inner);
        drop(previous);
        drop(replacement);
        result
    }
}
