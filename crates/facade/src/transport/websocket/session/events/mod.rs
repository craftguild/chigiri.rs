//! One existing outbox with normal, finite prior-event, or cancelled authority.

mod one_shot;
mod sending;
#[cfg(test)]
mod tests;

use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_runtime::GraphQlResponse;

use super::WebSocketWireFrame;
use crate::subscription::bridge_resources::{
    BridgeEmissionLease, BridgeGeneration, WebSocketOperationPhysicalLease,
};
use crate::subscription::outbox::{
    SubscriptionEventReservation, SubscriptionOutbox, SubscriptionOutboxSignal,
};
use crate::subscription::worker::websocket::WebSocketTerminalCarrier;
pub(super) use one_shot::WebSocketOneShotEventFailure;
use sending::EventSend;

enum EventAuthority {
    Active(BridgeEmissionLease),
    PriorEvents,
    Cancelled,
}

/// Contains no source or application callback and cannot create another queue.
pub(super) struct WebSocketEventOutbox {
    outbox: Option<SubscriptionOutbox>,
    physical: Option<WebSocketOperationPhysicalLease>,
    generation: BridgeGeneration,
    authority: EventAuthority,
    sending: Option<EventSend>,
    send_timeout: Duration,
}

impl WebSocketEventOutbox {
    pub(super) fn new(
        outbox: SubscriptionOutbox,
        physical: WebSocketOperationPhysicalLease,
        emission: BridgeEmissionLease,
        send_timeout: Duration,
    ) -> Result<Self, SubscriptionOutboxSignal> {
        if outbox.transport() != chigiri_http::SubscriptionTransport::WebSocket
            || physical.generation() != emission.generation()
            || !emission.can_emit()
        {
            return Err(SubscriptionOutboxSignal::Invariant);
        }
        Ok(Self {
            outbox: Some(outbox),
            generation: physical.generation(),
            physical: Some(physical),
            authority: EventAuthority::Active(emission),
            sending: None,
            send_timeout,
        })
    }

    pub(super) fn reserve_before_poll(
        &mut self,
        now: u64,
    ) -> Result<SubscriptionEventReservation, SubscriptionOutboxSignal> {
        self.require_active()?;
        self.outbox_mut()?.reserve_before_poll(now)
    }

    pub(super) fn observation_outbox(&mut self) -> Option<&mut SubscriptionOutbox> {
        self.outbox.as_mut()
    }

    pub(super) fn freeze_terminal_counter(&mut self) {
        if let Some(outbox) = &mut self.outbox {
            outbox.freeze_terminal_counter();
        }
    }

    pub(super) fn encode_and_enqueue(
        &mut self,
        reservation: SubscriptionEventReservation,
        response: &GraphQlResponse,
        now: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        self.require_active()?;
        self.outbox_mut()?
            .encode_and_enqueue(reservation, response, now)
    }

    /// Proof of an already revoked matching generation creates no new events.
    ///
    /// The caller retires before taking the terminal message. Explicit client
    /// cancellation has no drain authority and must call invalidate instead.
    pub(super) fn retire(
        &mut self,
        terminal: &WebSocketTerminalCarrier,
    ) -> Result<(), SubscriptionOutboxSignal> {
        let EventAuthority::Active(emission) = &self.authority else {
            return Err(SubscriptionOutboxSignal::Invariant);
        };
        if terminal.generation() != self.generation
            || !terminal.permits_prior_event_drain()
            || emission.can_emit()
        {
            return Err(SubscriptionOutboxSignal::Invariant);
        }
        self.authority = EventAuthority::PriorEvents;
        Ok(())
    }

    /// Same-ID replacement and client completion stop every unsent old event.
    ///
    /// The original accepted frame is not truncated. Convert its payload-free
    /// progress before cancelling the common outbox's normal progress authority.
    pub(super) fn invalidate(&mut self) {
        self.authority = EventAuthority::Cancelled;
        if let Some(sending) = &mut self.sending {
            sending.retire(self.send_timeout);
        }
        if let Some(outbox) = &mut self.outbox {
            outbox.cancel();
        }
        self.physical.take();
    }

    pub(super) fn has_event(&self) -> bool {
        self.sending.is_none()
            && self.can_emit_prior_or_active()
            && self
                .outbox
                .as_ref()
                .is_some_and(SubscriptionOutbox::has_queued_events)
    }

    /// Called only after socket readiness and the root same-ID generation fence.
    ///
    /// No await or input/replacement admission may interleave this transition
    /// with start_send. Acquiring early would make a wholly unsent event physical.
    pub(super) fn begin_send(
        &mut self,
        now: u64,
    ) -> Result<Option<WebSocketWireFrame>, SubscriptionOutboxSignal> {
        if self.sending.is_some() || !self.can_emit_prior_or_active() {
            return Ok(None);
        }
        let Some((frame, progress)) = self.outbox_mut()?.begin_send(now)? else {
            return Ok(None);
        };
        let physical = self
            .physical
            .as_ref()
            .ok_or(SubscriptionOutboxSignal::Invariant)?
            .clone();
        let frame = WebSocketWireFrame::event(frame, physical);
        self.sending = Some(EventSend::new(&frame, progress));
        Ok(Some(frame))
    }

    /// Only the sole writer may reject its newly acquired, wholly unstarted
    /// frame. Normal cancellation must retain progress already handed to I/O.
    pub(super) fn discard_unstarted_send(&mut self) {
        self.sending.take();
    }

    pub(super) fn record_progress(
        &mut self,
        accepted: usize,
        flushed: bool,
        now: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        // Common queue and original partial-send expiry win ready progress.
        match self.on_timer(now) {
            Ok(()) | Err(SubscriptionOutboxSignal::Wait) => {}
            Err(error) => return Err(error),
        }
        self.sending
            .as_mut()
            .ok_or(SubscriptionOutboxSignal::Invariant)?
            .accept(accepted, flushed, now)?;
        if !matches!(self.authority, EventAuthority::Cancelled)
            && let Some(outbox) = &mut self.outbox
            && let Err(error) = outbox.on_progress(now)
        {
            self.invalidate();
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn poll_finish_send(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
    ) -> Poll<Result<bool, SubscriptionOutboxSignal>> {
        let Some(sending) = &self.sending else {
            return Poll::Ready(Ok(false));
        };
        match sending.poll_released(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }
        if let Some(progress) = self.sending.take().expect("released event send").finish() {
            self.outbox_mut()?.finish_send(progress, now)?;
        }
        Poll::Ready(Ok(true))
    }

    pub(super) fn next_deadline(&mut self) -> Result<Option<u64>, SubscriptionOutboxSignal> {
        if matches!(self.authority, EventAuthority::Cancelled) {
            return self
                .sending
                .as_ref()
                .map_or(Ok(None), EventSend::next_deadline);
        }
        match &mut self.outbox {
            Some(outbox) => outbox.next_deadline(),
            None => self
                .sending
                .as_ref()
                .map_or(Ok(None), EventSend::next_deadline),
        }
    }

    pub(super) fn on_timer(&mut self, now: u64) -> Result<(), SubscriptionOutboxSignal> {
        if matches!(self.authority, EventAuthority::Cancelled) {
            return self
                .sending
                .as_ref()
                .map_or(Ok(()), |sending| sending.on_timer(now));
        }
        let result = match &mut self.outbox {
            Some(outbox) => outbox.on_timer(now),
            None => self
                .sending
                .as_ref()
                .map_or(Ok(()), |sending| sending.on_timer(now)),
        };
        if let Err(error) = result
            && error != SubscriptionOutboxSignal::Wait
        {
            self.invalidate();
        }
        result
    }

    /// Terminal staging waits for every earlier admitted physical event.
    pub(super) fn is_drained(&self) -> bool {
        self.sending.is_none()
            && self
                .outbox
                .as_ref()
                .is_none_or(SubscriptionOutbox::is_empty)
    }

    fn require_active(&self) -> Result<(), SubscriptionOutboxSignal> {
        match &self.authority {
            EventAuthority::Active(emission) if emission.can_emit() => Ok(()),
            _ => Err(SubscriptionOutboxSignal::Cancelled),
        }
    }

    fn can_emit_prior_or_active(&self) -> bool {
        match &self.authority {
            EventAuthority::Active(emission) => emission.can_emit(),
            EventAuthority::PriorEvents => true,
            EventAuthority::Cancelled => false,
        }
    }

    fn outbox_mut(&mut self) -> Result<&mut SubscriptionOutbox, SubscriptionOutboxSignal> {
        self.outbox
            .as_mut()
            .ok_or(SubscriptionOutboxSignal::Cancelled)
    }
}
