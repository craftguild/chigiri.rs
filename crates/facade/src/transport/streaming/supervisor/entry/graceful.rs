//! Stop demand, discard backlog, and preserve only eligible ordered work.

use chigiri_runtime::__private::SubscriptionTermination;

use super::Entry;

impl Entry {
    pub(super) fn cancel_managed(&mut self) {
        let outcome = self.port.control().and_then(|control| control.outcome());
        let (decision, error) = match outcome {
            Some(crate::ShutdownOutcome::InfrastructureFailure) => (
                SubscriptionTermination::Internal,
                Some(super::HttpStreamingError::Worker),
            ),
            Some(crate::ShutdownOutcome::Expired) => (
                SubscriptionTermination::ServerShutdown,
                Some(super::HttpStreamingError::TimedOut),
            ),
            _ => (SubscriptionTermination::ServerShutdown, None),
        };
        self.cancel_with(decision, error);
    }

    pub(in crate::transport::streaming::supervisor) fn begin_graceful(&mut self) {
        if self.draining || self.stopping {
            return;
        }
        self.draining = true;
        self.heartbeat.cancel();
        let skipped = self.outbox.discard_waiting();
        self.graceful_event = !skipped
            && self.pending_terminal.is_none()
            && self
                .events
                .as_ref()
                .is_some_and(|events| events.activity().is_active());
        // An idle or pending source never extends the drain. Revoking its
        // generation leaves the original concrete cleanup and charge intact.
        if !self.graceful_event {
            self.pending_terminal.take();
            self.finish_observation(SubscriptionTermination::ServerShutdown);
            if let Some(events) = &mut self.events {
                events.cancel();
            }
        }
    }

    pub(super) fn finish_graceful(&mut self) {
        if !self.draining
            || self.graceful_event
            || self.current.is_some()
            || !self.outbox.is_empty()
        {
            return;
        }
        self.finish_observation(SubscriptionTermination::ServerShutdown);
        if let Some(events) = &mut self.events {
            events.cancel();
        }
        self.wire_closed = true;
        self.port.finish(None);
    }
}
