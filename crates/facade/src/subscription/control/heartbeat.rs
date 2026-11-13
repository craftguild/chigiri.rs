//! Coalesced HTTP-stream heartbeat ownership between complete entries.

use std::sync::Arc;

use chigiri_http::{
    __encode_subscription_control, __subscription_static_control_len, SubscriptionControl,
    SubscriptionStaticControl, SubscriptionTransport,
};

use super::super::delivery::EndpointByteOwner;
use super::encoding::encode_reserved;
use super::frame::InFlightGate;
use super::{SubscriptionControlFrame, SubscriptionControlSignal};

/// A single coalesced heartbeat marker for one SSE or multipart stream.
pub struct HttpHeartbeatStaging {
    bytes: EndpointByteOwner,
    transport: SubscriptionTransport,
    pending: bool,
    in_flight: Arc<InFlightGate>,
    terminal: Option<SubscriptionControlSignal>,
}

impl HttpHeartbeatStaging {
    pub(crate) fn new(
        bytes: EndpointByteOwner,
        transport: SubscriptionTransport,
    ) -> Result<Self, SubscriptionControlSignal> {
        if !matches!(
            transport,
            SubscriptionTransport::ServerSentEvents | SubscriptionTransport::Multipart
        ) {
            return Err(SubscriptionControlSignal::InvalidTransportArguments);
        }
        Ok(Self {
            bytes,
            transport,
            pending: false,
            in_flight: Arc::new(InFlightGate::default()),
            terminal: None,
        })
    }

    /// Coalesces any number of timer notifications into one pending marker.
    pub fn mark_due(&mut self) -> Result<(), SubscriptionControlSignal> {
        self.ensure_live()?;
        self.pending = true;
        Ok(())
    }

    /// Encodes a heartbeat only when events and a physical write are absent.
    ///
    /// `event_ready` gives a ready event priority. `write_entry_live` must stay
    /// true while any complete event or control frame is physically retained,
    /// including after all of its bytes have been accepted.
    pub fn try_begin(
        &mut self,
        event_ready: bool,
        write_entry_live: bool,
    ) -> Result<Option<SubscriptionControlFrame>, SubscriptionControlSignal> {
        self.ensure_live()?;
        if !self.pending || event_ready || write_entry_live || self.in_flight.is_busy() {
            return Ok(None);
        }
        let Some(lease) = self.in_flight.try_acquire() else {
            return Ok(None);
        };
        let control = SubscriptionStaticControl::Heartbeat;
        let transport = self.transport;
        let length = __subscription_static_control_len(transport, control)
            .map_err(|error| self.fail(SubscriptionControlSignal::from_encoding(error)))?;
        let staged = encode_reserved(&self.bytes, length, Some(length), |maximum| {
            __encode_subscription_control(transport, SubscriptionControl::Heartbeat, maximum)
        })
        .map_err(|signal| self.fail(signal))?;
        let (encoded, charge) = staged.into_parts();
        let charge = charge.into_in_flight();
        if charge.bytes() != encoded.len() {
            return Err(self.fail(SubscriptionControlSignal::Accounting));
        }
        self.pending = false;
        Ok(Some(SubscriptionControlFrame::new(encoded, charge, lease)))
    }

    /// Returns whether one uncharged timer marker is pending.
    pub const fn is_pending(&self) -> bool {
        self.pending
    }

    /// Cancels the marker without releasing an already returned physical frame.
    pub fn cancel(&mut self) {
        self.pending = false;
        self.terminal
            .get_or_insert(SubscriptionControlSignal::Cancelled);
    }

    fn ensure_live(&self) -> Result<(), SubscriptionControlSignal> {
        self.terminal.map_or(Ok(()), Err)
    }

    fn fail(&mut self, signal: SubscriptionControlSignal) -> SubscriptionControlSignal {
        self.pending = false;
        *self.terminal.get_or_insert(signal)
    }
}
