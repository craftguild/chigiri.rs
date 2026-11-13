//! Select one safe failure outside locks, then wait for the prior physical owner.

use chigiri_runtime::GraphQlResponse;

use super::{CurrentFrame, Entry, HttpStreamingError};
use crate::subscription::control::HttpSubscriptionTerminal;
use crate::subscription::outbox::SubscriptionOutboxSignal;

pub(super) struct PendingFailure {
    response: GraphQlResponse,
    counter: Option<u64>,
    error: HttpStreamingError,
}

impl Entry {
    pub(in crate::transport::streaming::supervisor) fn fail(&mut self, error: HttpStreamingError) {
        if self.failure_selected || self.wire_closed {
            self.cancel(Some(error));
            return;
        }
        self.failure_selected = true;
        let decision = match error {
            HttpStreamingError::TimedOut => {
                chigiri_runtime::__private::SubscriptionTermination::ExecutionFailed
            }
            HttpStreamingError::Delivery(signal) => {
                crate::subscription::failure::outbox_termination(signal)
            }
            HttpStreamingError::Control(signal) => {
                crate::subscription::failure::control_termination(signal)
            }
            _ => chigiri_runtime::__private::SubscriptionTermination::Internal,
        };
        self.finish_observation(decision);
        // The observer and potentially large response destruction never run
        // under the registry, mailbox, or acceptance timeline locks.
        let response = self.configuration.failure.response(error);
        let counter = self.outbox.delivery_counter();
        self.stopping = true;
        self.outbox.cancel();
        self.heartbeat.cancel();
        self.pending_terminal.take();
        self.terminal.take();
        if let Some(events) = &mut self.events {
            events.cancel();
        }
        if error == HttpStreamingError::Delivery(SubscriptionOutboxSignal::SendStalled) {
            self.cancel(Some(error));
            return;
        }
        self.current = self.current.take().map(|current| match current {
            CurrentFrame::Event(progress) => {
                CurrentFrame::RetiredEvent(progress.retire(self.configuration.send_no_progress))
            }
            other => other,
        });
        self.failure = Some(PendingFailure {
            response,
            counter,
            error,
        });
    }

    pub(super) fn stage_failure(&mut self) {
        if self.current.is_some() {
            return;
        }
        let Some(failure) = self.failure.take() else {
            return;
        };
        let reservation = self
            .configuration
            .bytes
            .try_reserve(self.configuration.maximum_event_bytes.min(64 * 1024));
        let composed = reservation
            .map_err(|_| failure.error)
            .and_then(|reservation| {
                let counter = (self.configuration.transport
                    == chigiri_http::SubscriptionTransport::ServerSentEvents)
                    .then_some(failure.counter)
                    .flatten();
                HttpSubscriptionTerminal::new(
                    reservation,
                    self.configuration.bytes.clone(),
                    self.configuration.transport,
                    counter,
                )
                .error(&failure.response)
                .map_err(HttpStreamingError::Control)
            });
        match composed {
            Ok(terminal) => self.terminal = Some(terminal),
            Err(_) => self.cancel(Some(failure.error)),
        }
    }
}
