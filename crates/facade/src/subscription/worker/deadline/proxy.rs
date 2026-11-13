//! Generation-bound task proxies that never own timer futures.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::DeadlineRole;
use super::state::DeadlineState;
use crate::subscription::worker::error::SubscriptionWorkerRuntimeError;

/// A task-side view of one externally polled exact deadline generation.
pub(crate) struct MonitoredDeadline {
    state: Arc<DeadlineState>,
    role: DeadlineRole,
    generation: u64,
    live: bool,
    activity: Option<crate::managed::EventActivity>,
}

/// Two already-armed inherited budgets polled with stable precedence.
pub(crate) struct MonitoredDeadlinePair {
    first: MonitoredDeadline,
    second: MonitoredDeadline,
}

impl MonitoredDeadlinePair {
    pub(super) fn new(first: MonitoredDeadline, second: MonitoredDeadline) -> Self {
        Self { first, second }
    }

    pub(crate) fn clear(self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.first.clear()?;
        self.second.clear()
    }

    pub(crate) fn handoff_overall_and_clear_setup(
        self,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.second.clear()?;
        self.first.relinquish();
        Ok(())
    }
}

impl Future for MonitoredDeadlinePair {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if Pin::new(&mut self.first).poll(context).is_ready()
            || Pin::new(&mut self.second).poll(context).is_ready()
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl MonitoredDeadline {
    pub(crate) async fn registered(&mut self) -> Result<(), SubscriptionWorkerRuntimeError> {
        std::future::poll_fn(|context| {
            self.state
                .poll_registered(self.role, self.generation, context)
        })
        .await
    }

    pub(super) fn new(state: Arc<DeadlineState>, role: DeadlineRole, generation: u64) -> Self {
        Self {
            state,
            role,
            generation,
            live: true,
            activity: None,
        }
    }

    pub(super) fn with_task(mut self, activity: Option<crate::managed::EventActivity>) -> Self {
        self.activity = activity;
        self
    }

    pub(crate) fn task_activity(&self) -> Option<crate::managed::EventActivity> {
        self.activity.clone()
    }

    pub(crate) fn clear(mut self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.state.clear(self.role, self.generation)?;
        self.live = false;
        Ok(())
    }

    /// Drops only this task-side view while preserving the original externally
    /// polled timer for the next phase. This never clears or re-arms a budget.
    pub(crate) fn relinquish(mut self) {
        self.live = false;
    }
}

impl Future for MonitoredDeadline {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let state = Arc::clone(&self.state);
        let role = self.role;
        let generation = self.generation;
        state.poll_proxy(role, generation, &mut self.live, context)
    }
}

impl Drop for MonitoredDeadline {
    fn drop(&mut self) {
        if self.live {
            self.state.abandon(self.role, self.generation);
            self.live = false;
        }
    }
}
