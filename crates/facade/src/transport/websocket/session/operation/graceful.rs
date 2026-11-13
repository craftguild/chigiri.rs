//! A finite execution or one active event may finish; waiting sources cannot.

use std::task::Poll;

use super::failure::WebSocketOperationSharedFailure;
use super::state::{Phase, WebSocketOperationSlot, WebSocketOperationTransition};
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::worker::websocket::ActiveWebSocketSubscription;
use crate::transport::websocket::session::events::WebSocketEventOutbox;

impl<C: Send + Sync + 'static> WebSocketOperationSlot<C> {
    #[cfg(test)]
    pub(in crate::transport::websocket::session) fn setup_finished_for_test(&self) -> bool {
        match &self.phase {
            Phase::Validation(worker) => worker.is_finished_for_test(),
            Phase::SubscriptionSetup(worker) => worker.is_finished_for_test(),
            _ => false,
        }
    }

    pub(in crate::transport::websocket::session) fn is_finite(&self) -> bool {
        self.finite
    }

    /// A frame selected before observing Graceful still belongs to the waiting
    /// backlog if endpoint authorization rejects its first native handoff.
    pub(in crate::transport::websocket::session) fn discard_unstarted_event(
        &mut self,
        controls: &mut WebSocketControlStaging,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        if let Some(events) = &mut self.events {
            events.discard_unstarted_send();
        }
        self.draining = true;
        self.graceful_event = false;
        self.cancel_inner(
            controls,
            chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
        )
    }

    pub(in crate::transport::websocket::session) fn begin_graceful(
        &mut self,
        controls: &mut WebSocketControlStaging,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        if self.draining {
            return Ok(());
        }
        self.draining = true;
        if self.finite {
            return Ok(());
        }
        let skipped = self
            .events
            .as_mut()
            .and_then(WebSocketEventOutbox::observation_outbox)
            .is_some_and(|outbox| outbox.discard_waiting());
        self.graceful_event = !skipped
            && matches!(
                &self.phase,
                Phase::Subscription { worker, .. } if worker.activity.is_active()
            );
        if !self.graceful_event {
            self.cancel_inner(
                controls,
                chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
            )?;
        }
        Ok(())
    }

    pub(super) fn poll_graceful_subscription(
        &mut self,
        worker: ActiveWebSocketSubscription,
        demand_outstanding: bool,
    ) -> Result<Poll<WebSocketOperationTransition>, WebSocketOperationSharedFailure> {
        if self
            .events
            .as_ref()
            .is_none_or(WebSocketEventOutbox::is_drained)
        {
            let outbox = self
                .events
                .as_mut()
                .and_then(WebSocketEventOutbox::observation_outbox);
            self.phase = Phase::Terminating(worker.cancel(
                chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
                outbox,
            ));
            return Ok(Poll::Ready(WebSocketOperationTransition::Progress));
        }
        self.phase = Phase::Subscription {
            worker,
            demand_outstanding,
        };
        Ok(Poll::Pending)
    }
}
