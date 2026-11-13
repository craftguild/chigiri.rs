//! Retiring source demand and retaining its final delivery-counter snapshot.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::{SubscriptionEventReservation, SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::control::HttpSubscriptionTerminal;

impl SubscriptionOutbox {
    /// Retires an empty HTTP outbox with its exact outstanding source reservation.
    ///
    /// Call only for natural source completion or a safe terminal source error,
    /// after earlier event frames have physically finished. Cancellation emits
    /// nothing and instead drops the reservation and cancels this outbox.
    pub fn finish_http_source(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<HttpSubscriptionTerminal, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        if self.candidate.is_some() {
            return Err(self.terminate(SubscriptionOutboxSignal::Invariant));
        }
        if self.transport == SubscriptionTransport::WebSocket {
            return Err(self.terminate(SubscriptionOutboxSignal::InvalidTransportArguments));
        }
        self.freeze_source_drops()?;
        let controller = self
            .controller
            .take()
            .ok_or(SubscriptionOutboxSignal::Invariant)?;
        self.terminal.set(SubscriptionOutboxSignal::Cancelled);
        let (reservation, bytes, dropped) = controller
            .into_terminal(reservation.inner)
            .map_err(SubscriptionOutboxSignal::from_decision)?;
        self.last_dropped_total = dropped;
        let counter = if self.transport == SubscriptionTransport::ServerSentEvents {
            self.delivery_counter()
        } else {
            None
        };
        Ok(HttpSubscriptionTerminal::new(
            reservation,
            bytes,
            self.transport,
            counter,
        ))
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) const fn transport(&self) -> SubscriptionTransport {
        self.transport
    }

    pub(crate) fn delivery_counter(&self) -> Option<u64> {
        (self.overflow == SubscriptionOverflow::DropOldest).then(|| {
            self.controller
                .as_ref()
                .map_or(self.last_dropped_total, |controller| {
                    controller.dropped_total()
                })
        })
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn is_empty(&self) -> bool {
        self.candidate.is_none()
            && self
                .controller
                .as_ref()
                .is_none_or(|controller| controller.is_empty())
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn has_queued_events(&self) -> bool {
        self.candidate.is_some()
            || self
                .controller
                .as_ref()
                .is_some_and(|controller| controller.has_queued_events())
    }
}
