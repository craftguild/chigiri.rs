//! Sequential outbox state and common-policy transition orchestration.

use std::sync::Arc;

use chigiri_http::{
    __encode_subscription_event, EncodedSubscriptionEvent, FinalizedSubscriptionEvent,
    SubscriptionTransport,
};
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow};

use super::super::delivery::{
    ControllerReservation, DeliveryController, DeliveryDecision, EndpointByteOwner,
    MonotonicInstant,
};
use super::super::settings::ValidatedSubscriptionSettings;
use super::frame::{SharedClock, SharedTerminal};
#[cfg(any(test, feature = "subscription-worker"))]
use super::observations::DropObservationState;
use super::{SubscriptionOutboxSignal, SubscriptionSendFrame, SubscriptionSendProgress};

pub(super) type EventController =
    DeliveryController<EncodedSubscriptionEvent, FinalizedSubscriptionEvent>;

/// A move-only maximum-size reservation acquired before source demand.
pub struct SubscriptionEventReservation {
    pub(super) inner: ControllerReservation,
}

/// One transport-neutral, bounded subscription event outbox.
///
/// Official bridges must reserve before polling a source, then consume that
/// reservation to encode and enqueue exactly one result.
pub struct SubscriptionOutbox {
    pub(super) controller: Option<EventController>,
    pub(super) overflow: SubscriptionOverflow,
    pub(super) transport: SubscriptionTransport,
    operation_id: Option<Arc<str>>,
    maximum_event_bytes: usize,
    pub(super) clock: Arc<SharedClock>,
    pub(super) terminal: Arc<SharedTerminal>,
    pub(super) last_dropped_total: u64,
    pub(super) candidate: Option<super::candidate::EncodedCandidate>,
    pub(super) terminal_counter_frozen: bool,
    pub(super) source_delivery: Option<Arc<chigiri_runtime::__private::HostSourceDelivery>>,
    pub(super) source_dropped: u64,
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(super) drop_observations: DropObservationState,
}

impl SubscriptionOutbox {
    pub(crate) fn new(
        settings: &ValidatedSubscriptionSettings,
        bytes: EndpointByteOwner,
        overflow: SubscriptionOverflow,
        transport: SubscriptionTransport,
        operation_id: Option<Arc<str>>,
    ) -> Result<Self, SubscriptionOutboxSignal> {
        if !valid_transport_arguments(transport, operation_id.as_deref()) {
            return Err(SubscriptionOutboxSignal::InvalidTransportArguments);
        }
        let controller = DeliveryController::new(settings, bytes, overflow);
        match controller.evaluate_admission(MonotonicInstant::from_nanos(0), false) {
            DeliveryDecision::Admit => {}
            decision => return Err(SubscriptionOutboxSignal::from_decision(decision)),
        }
        Ok(Self {
            controller: Some(controller),
            overflow,
            transport,
            operation_id,
            maximum_event_bytes: settings.limits().max_encoded_event_bytes,
            clock: Arc::new(SharedClock::default()),
            terminal: Arc::new(SharedTerminal::default()),
            last_dropped_total: 0,
            candidate: None,
            terminal_counter_frozen: false,
            source_delivery: None,
            source_dropped: 0,
            #[cfg(any(test, feature = "subscription-worker"))]
            drop_observations: DropObservationState::default(),
        })
    }

    /// Acquires the one maximum-size byte reservation before source polling.
    pub fn reserve_before_poll(
        &mut self,
        now_ns: u64,
    ) -> Result<SubscriptionEventReservation, SubscriptionOutboxSignal> {
        self.require_source_demand()?;
        let now = self.observe_time(now_ns)?;
        self.admit_candidate(now)?;
        if self.candidate.is_some() {
            return Err(SubscriptionOutboxSignal::Wait);
        }
        loop {
            let result = self.controller_mut()?.reserve_before_poll(now);
            match result {
                Ok(inner) => return Ok(SubscriptionEventReservation { inner }),
                Err(DeliveryDecision::EvictWhole(count)) => self.evict(count)?,
                Err(DeliveryDecision::Pause) => return Err(SubscriptionOutboxSignal::Wait),
                Err(decision) => return Err(self.terminate_decision(decision)),
            }
        }
    }

    /// Bounded-encodes and retains exactly one source result until admission.
    ///
    /// A candidate blocked by a non-evictable physical event keeps its existing
    /// exact encoded charge. It is not queued, evictable, or another source demand.
    pub fn encode_and_enqueue(
        &mut self,
        mut reservation: SubscriptionEventReservation,
        response: &GraphQlResponse,
        now_ns: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        self.require_source_demand()?;
        let now = self.observe_time(now_ns)?;
        if self.candidate.is_some() {
            return Err(self.terminate(SubscriptionOutboxSignal::Invariant));
        }
        let encoded = __encode_subscription_event(
            self.transport,
            self.operation_id.as_deref(),
            response,
            self.maximum_event_bytes,
        )
        .map_err(|error| self.terminate_encoding(error))?;
        self.controller_mut()?
            .shrink_encoded_candidate(&mut reservation.inner, encoded.len())
            .map_err(|decision| self.terminate_decision(decision))?;
        self.candidate = Some(super::candidate::EncodedCandidate::new(
            reservation,
            encoded,
        ));
        self.admit_candidate(now)
    }

    /// Moves one queued event into immutable in-flight ownership.
    pub fn begin_send(
        &mut self,
        now_ns: u64,
    ) -> Result<Option<(SubscriptionSendFrame, SubscriptionSendProgress)>, SubscriptionOutboxSignal>
    {
        let now = self.observe_time(now_ns)?;
        self.admit_candidate(now)?;
        self.evaluate_dequeue(now)?;
        let overflow = self.overflow;
        let mut encoding_failure = None;
        let result = self
            .controller_mut()?
            .begin_send_with(now, |encoded, dropped_total| {
                let counter = match overflow {
                    SubscriptionOverflow::Backpressure => None,
                    SubscriptionOverflow::DropOldest => Some(dropped_total),
                };
                encoded.finalize(counter).map_err(|error| {
                    let signal = SubscriptionOutboxSignal::from_encoding(error);
                    encoding_failure = Some(signal);
                    signal.decision_for_encoding()
                })
            });
        let Some(mut send) = result.map_err(|decision| match encoding_failure {
            Some(signal) => self.terminate(signal),
            None => self.terminate_decision(decision),
        })?
        else {
            return Ok(None);
        };
        if send.bytes() != send.payload().len() {
            return Err(self.terminate(SubscriptionOutboxSignal::Accounting));
        }
        let progress = send
            .take_progress()
            .ok_or_else(|| self.terminate(SubscriptionOutboxSignal::Invariant))?;
        Ok(Some((
            SubscriptionSendFrame::new(send),
            SubscriptionSendProgress::new(
                progress,
                Arc::clone(&self.clock),
                Arc::clone(&self.terminal),
            ),
        )))
    }

    /// Re-evaluates all delivery progress conditions after a writer attempt.
    pub fn on_progress(&mut self, now_ns: u64) -> Result<(), SubscriptionOutboxSignal> {
        let now = self.observe_time(now_ns)?;
        loop {
            match self.controller_ref()?.evaluate_progress(now) {
                DeliveryDecision::Admit => return self.admit_candidate(now),
                DeliveryDecision::EvictWhole(count) => self.evict(count)?,
                DeliveryDecision::Pause => {
                    return Err(self.terminate(SubscriptionOutboxSignal::Invariant));
                }
                decision => return Err(self.terminate_decision(decision)),
            }
        }
    }

    /// Applies every armed common-policy deadline when the bridge timer fires.
    pub fn on_timer(&mut self, now_ns: u64) -> Result<(), SubscriptionOutboxSignal> {
        let now = self.observe_time(now_ns)?;
        loop {
            match self.controller_ref()?.evaluate_timer(now) {
                DeliveryDecision::Admit => return self.admit_candidate(now),
                DeliveryDecision::EvictWhole(count) => self.evict(count)?,
                DeliveryDecision::Pause => return Err(SubscriptionOutboxSignal::Wait),
                decision => return Err(self.terminate_decision(decision)),
            }
        }
    }

    /// Clears in-flight ownership after all bytes and the exact frame owner end.
    pub fn finish_send(
        &mut self,
        progress: SubscriptionSendProgress,
        now_ns: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        self.on_progress(now_ns)?;
        self.controller_mut()?
            .finish_send(progress.inner)
            .map_err(|decision| self.terminate_decision(decision))?;
        self.admit_candidate(MonotonicInstant::from_nanos(now_ns))
    }

    /// Returns the earliest common queue, send, or pressure deadline in nanos.
    pub fn next_deadline(&mut self) -> Result<Option<u64>, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        self.controller_ref()?
            .next_deadline()
            .map(|deadline| deadline.map(MonotonicInstant::as_nanos))
            .map_err(|decision| self.terminate_decision(decision))
    }

    /// Cancels this generation and releases all internally owned queue state.
    pub fn cancel(&mut self) {
        self.terminate(SubscriptionOutboxSignal::Cancelled);
    }

    /// Takes each successful whole-event eviction total at most once.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn take_drop_observations(&mut self) -> super::SubscriptionDropTotals {
        let _ = self.sync_source_drops();
        self.drop_observations.take()
    }

    fn evaluate_dequeue(&mut self, now: MonotonicInstant) -> Result<(), SubscriptionOutboxSignal> {
        loop {
            match self.controller_ref()?.evaluate_dequeue(now) {
                DeliveryDecision::Admit => return Ok(()),
                DeliveryDecision::EvictWhole(count) => self.evict(count)?,
                DeliveryDecision::Pause => return Err(SubscriptionOutboxSignal::Wait),
                decision => return Err(self.terminate_decision(decision)),
            }
        }
    }

    pub(super) fn evict(&mut self, count: usize) -> Result<(), SubscriptionOutboxSignal> {
        if count == 0 {
            return Err(self.terminate(SubscriptionOutboxSignal::Invariant));
        }
        for _ in 0..count {
            let result = {
                let controller = self.controller_mut()?;
                controller
                    .evict_one()
                    .map(|evicted| evicted.then(|| controller.dropped_total()))
            };
            match result {
                Ok(Some(total)) => {
                    #[cfg(any(test, feature = "subscription-worker"))]
                    self.drop_observations.record(total);
                    #[cfg(not(any(test, feature = "subscription-worker")))]
                    let _ = total;
                }
                Ok(None) => return Err(self.terminate(SubscriptionOutboxSignal::Invariant)),
                Err(decision) => return Err(self.terminate_decision(decision)),
            }
        }
        Ok(())
    }
}

fn valid_transport_arguments(transport: SubscriptionTransport, operation_id: Option<&str>) -> bool {
    match (transport, operation_id) {
        (SubscriptionTransport::WebSocket, Some(id)) => !id.is_empty() && id.len() <= 256,
        (SubscriptionTransport::ServerSentEvents | SubscriptionTransport::Multipart, None) => true,
        _ => false,
    }
}
