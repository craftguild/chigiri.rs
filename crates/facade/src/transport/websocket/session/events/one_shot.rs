//! The sole pre-terminal one-shot result is an accepted event, not new demand.

use std::time::Duration;

use super::{EventAuthority, WebSocketEventOutbox};
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::websocket::{JoinedWebSocketOneShot, WebSocketTerminalCarrier};

/// A failed final encoding still owns the selected exact-generation terminal.
pub(in crate::transport::websocket::session) enum WebSocketOneShotEventFailure {
    Operation {
        signal: SubscriptionOutboxSignal,
        terminal: Box<WebSocketTerminalCarrier>,
    },
    SharedInvariant(SubscriptionWorkerRuntimeError),
}

impl WebSocketEventOutbox {
    /// Consumes only a joined worker's one already reserved result.
    ///
    /// The worker has revoked its logical ID. This closed transition may encode
    /// the result already accepted before that cutoff; it cannot create another
    /// reservation, accept an arbitrary response, or resume ordinary emission.
    pub(in crate::transport::websocket::session) fn from_one_shot(
        mut outbox: SubscriptionOutbox,
        mut joined: JoinedWebSocketOneShot,
        send_timeout: Duration,
        now: u64,
    ) -> Result<(Self, WebSocketTerminalCarrier), WebSocketOneShotEventFailure> {
        // A malformed internal joined result has no safe reusable event owner.
        // The worker constructor guarantees the event and terminal are present.
        let event = joined.take_event();
        let terminal = joined.take_terminal();
        let terminal = match terminal {
            Ok(terminal) => terminal,
            Err(error) => return Err(WebSocketOneShotEventFailure::SharedInvariant(error)),
        };
        let valid = outbox.transport() == chigiri_http::SubscriptionTransport::WebSocket
            && outbox.is_empty()
            && terminal.permits_prior_event_drain();
        let Some((reservation, response)) = event else {
            return Err(WebSocketOneShotEventFailure::Operation {
                signal: SubscriptionOutboxSignal::Invariant,
                terminal: Box::new(terminal),
            });
        };
        let Some(physical) = terminal.physical_lease() else {
            return Err(WebSocketOneShotEventFailure::Operation {
                signal: SubscriptionOutboxSignal::Invariant,
                terminal: Box::new(terminal),
            });
        };
        if !valid {
            return Err(WebSocketOneShotEventFailure::Operation {
                signal: SubscriptionOutboxSignal::Invariant,
                terminal: Box::new(terminal),
            });
        }
        if let Err(signal) = outbox.encode_and_enqueue(reservation, &response, now) {
            return Err(WebSocketOneShotEventFailure::Operation {
                signal,
                terminal: Box::new(terminal),
            });
        }
        let result = Self {
            outbox: Some(outbox),
            generation: terminal.generation(),
            physical: Some(physical),
            authority: EventAuthority::PriorEvents,
            sending: None,
            send_timeout,
        };
        Ok((result, terminal))
    }
}
