//! Short synchronous queue, reservation, timer, and counter transitions.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chigiri_runtime::SubscriptionOverflow;

use crate::subscription::settings::ValidatedSubscriptionSettings;

use super::controller_policy::ControllerLimits;
use super::queue::EventQueueError;
use super::{
    ByteReservationError, CheckedCounter, ControllerReservation, DeliveryDecision,
    EndpointByteOwner, EvaluationPoint, EventQueue, InFlightProgress, InFlightSend, InFlightSlot,
    MonotonicInstant, QueuedEvent, SharedPressureWait, TerminationReason,
};

/// Sequential transport-neutral state for one subscription generation.
pub(crate) struct DeliveryController<Queued, Finalized = Queued> {
    pub(super) overflow: SubscriptionOverflow,
    pub(super) limits: ControllerLimits,
    pub(super) bytes: EndpointByteOwner,
    reservation_held: Arc<AtomicBool>,
    pub(super) queue: EventQueue<Queued>,
    pub(super) in_flight: Option<InFlightSlot<Finalized>>,
    dropped: CheckedCounter,
    pub(super) evictions_frozen: bool,
    pub(super) pressure: SharedPressureWait,
    pub(super) accounting_failure: bool,
    pub(super) counter_exhausted: bool,
}

impl<Queued, Finalized> DeliveryController<Queued, Finalized> {
    pub(crate) fn include_source_drops(
        &mut self,
        amount: u64,
    ) -> Result<(u64, u64), DeliveryDecision> {
        let previous = self.dropped.value();
        if self.evictions_frozen {
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        let next = self.dropped.add(amount).map_err(|_| {
            self.counter_exhausted = true;
            DeliveryDecision::Terminate(TerminationReason::CounterExhausted)
        })?;
        Ok((previous, next))
    }
    pub(crate) fn new(
        settings: &ValidatedSubscriptionSettings,
        bytes: EndpointByteOwner,
        overflow: SubscriptionOverflow,
    ) -> Self {
        let limits = settings.limits();
        Self {
            overflow,
            limits: ControllerLimits {
                max_queued_events: limits.max_queued_events,
                max_queued_bytes: limits.max_queued_bytes,
                max_event_bytes: limits.max_encoded_event_bytes,
                max_queue_age: limits.max_queue_age,
                send_no_progress: limits.send_no_progress_timeout,
            },
            bytes,
            reservation_held: Arc::new(AtomicBool::new(false)),
            queue: EventQueue::new(),
            in_flight: None,
            dropped: CheckedCounter::new(),
            evictions_frozen: false,
            pressure: SharedPressureWait::new(),
            accounting_failure: false,
            counter_exhausted: false,
        }
    }

    /// Acquires the one maximum-size reservation before source demand.
    pub(crate) fn reserve_before_poll(
        &mut self,
        now: MonotonicInstant,
    ) -> Result<ControllerReservation, DeliveryDecision> {
        if self.reservation_held.load(Ordering::Acquire) {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        if self.pressure.is_expired(now) {
            return Err(DeliveryDecision::Terminate(
                TerminationReason::SharedBudgetPressureExpired,
            ));
        }
        // The maximum reservation is endpoint encoding capacity, not a real
        // queued event. DropOldest evaluates local pressure only once the
        // yielded event has a known encoded size. Backpressure still requires
        // worst-case space before demand because it cannot discard the item.
        let candidate = (self.overflow == SubscriptionOverflow::Backpressure)
            .then_some(self.limits.max_event_bytes);
        let decision =
            self.decision_without_pressure(EvaluationPoint::Reservation, now, false, candidate);
        if decision != DeliveryDecision::Admit {
            return Err(decision);
        }
        match self.bytes.try_reserve(self.limits.max_event_bytes) {
            Ok(reservation) => {
                self.pressure.note_success();
                self.reservation_held.store(true, Ordering::Release);
                Ok(ControllerReservation {
                    reservation: Some(reservation),
                    held: Arc::clone(&self.reservation_held),
                })
            }
            Err(ByteReservationError::Capacity) => {
                if self
                    .pressure
                    .note_failure(now, self.limits.max_queue_age)
                    .is_err()
                {
                    return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
                }
                Err(self.decision(EvaluationPoint::Reservation, now, false, candidate))
            }
            Err(ByteReservationError::Accounting | ByteReservationError::InvalidShrink) => {
                self.accounting_failure = true;
                Err(DeliveryDecision::Terminate(TerminationReason::Accounting))
            }
        }
    }

    /// Shrinks the same verified pre-demand token immediately after encoding.
    pub(crate) fn shrink_encoded_candidate(
        &mut self,
        reservation: &mut ControllerReservation,
        actual_bytes: usize,
    ) -> Result<(), DeliveryDecision> {
        if !Arc::ptr_eq(&reservation.held, &self.reservation_held) {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        reservation.shrink_encoded(actual_bytes).map_err(|error| {
            self.accounting_failure = true;
            let reason = match error {
                ByteReservationError::InvalidShrink => TerminationReason::OversizedEvent,
                ByteReservationError::Accounting | ByteReservationError::Capacity => {
                    TerminationReason::Accounting
                }
            };
            DeliveryDecision::Terminate(reason)
        })
    }

    /// Evaluates an exact encoded candidate before moving it into the queue.
    pub(crate) fn evaluate_candidate(
        &self,
        now: MonotonicInstant,
        actual_bytes: usize,
    ) -> DeliveryDecision {
        self.decision_without_pressure(EvaluationPoint::Enqueue, now, false, Some(actual_bytes))
    }

    pub(crate) fn enqueue(
        &mut self,
        reservation: ControllerReservation,
        actual_bytes: usize,
        payload: Queued,
        now: MonotonicInstant,
    ) -> Result<(), DeliveryDecision> {
        if !Arc::ptr_eq(&reservation.held, &self.reservation_held) {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        let charge = reservation.commit(actual_bytes).map_err(|error| {
            self.accounting_failure = true;
            match error {
                ByteReservationError::InvalidShrink => {
                    DeliveryDecision::Terminate(TerminationReason::OversizedEvent)
                }
                ByteReservationError::Accounting | ByteReservationError::Capacity => {
                    DeliveryDecision::Terminate(TerminationReason::Accounting)
                }
            }
        })?;
        self.queue
            .push(QueuedEvent::new(now, payload, charge))
            .map_err(|_| {
                self.accounting_failure = true;
                DeliveryDecision::Terminate(TerminationReason::Accounting)
            })
    }

    /// Detaches unstarted backlog without classifying shutdown as eviction.
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn discard_unstarted(&mut self) -> EventQueue<Queued> {
        std::mem::replace(&mut self.queue, EventQueue::new())
    }

    /// Evicts one whole FIFO event and increments the cumulative count once.
    pub(crate) fn evict_one(&mut self) -> Result<bool, DeliveryDecision> {
        if self.evictions_frozen {
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        if self.queue.is_empty() {
            return Ok(false);
        }
        let (dropped, queue) = (&mut self.dropped, &mut self.queue);
        let event = dropped
            .increment_with(|| queue.evict_oldest()?.ok_or(EventQueueError::Accounting))
            .map_err(|_| {
                self.counter_exhausted = true;
                DeliveryDecision::Terminate(TerminationReason::CounterExhausted)
            })?
            .map_err(|_| {
                self.accounting_failure = true;
                DeliveryDecision::Terminate(TerminationReason::Accounting)
            })?;
        drop(event);
        Ok(true)
    }

    pub(crate) fn begin_send_with(
        &mut self,
        now: MonotonicInstant,
        finalize: impl FnOnce(Queued, u64) -> Result<Finalized, DeliveryDecision>,
    ) -> Result<Option<InFlightSend<Finalized>>, DeliveryDecision> {
        if self.in_flight.is_some() {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        let Some(event) = self.queue.take_oldest().map_err(|_| {
            self.accounting_failure = true;
            DeliveryDecision::Terminate(TerminationReason::Accounting)
        })?
        else {
            return Ok(None);
        };
        let (slot, send) = event.into_in_flight_with(now, self.dropped.value(), finalize)?;
        self.in_flight = Some(slot);
        Ok(Some(send))
    }

    pub(crate) fn finish_send(
        &mut self,
        progress: InFlightProgress,
    ) -> Result<(), DeliveryDecision> {
        let Some(slot) = self.in_flight.as_ref() else {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        };
        if !slot.matches(&progress) {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        let (accepted, _) = slot.progress().map_err(|_| {
            self.accounting_failure = true;
            DeliveryDecision::Terminate(TerminationReason::Accounting)
        })?;
        if accepted != slot.bytes() {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        if !slot.sender_released() {
            self.accounting_failure = true;
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        self.in_flight.take();
        Ok(())
    }

    /// Retires an empty HTTP outbox using its exact outstanding source demand.
    pub(crate) fn into_terminal(
        self,
        reservation: ControllerReservation,
    ) -> Result<(super::EncodeReservation, EndpointByteOwner, u64), DeliveryDecision> {
        if !Arc::ptr_eq(&reservation.held, &self.reservation_held)
            || !self.queue.is_empty()
            || self.in_flight.is_some()
            || self.accounting_failure
            || self.counter_exhausted
        {
            return Err(DeliveryDecision::Terminate(TerminationReason::Invariant));
        }
        let reservation = reservation
            .into_encoding_reservation()
            .map_err(|_| DeliveryDecision::Terminate(TerminationReason::Accounting))?;
        Ok((reservation, self.bytes.clone(), self.dropped.value()))
    }

    pub(crate) const fn dropped_total(&self) -> u64 {
        self.dropped.value()
    }

    /// Freezes the exact terminal counter without releasing prior send state.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn freeze_evictions(&mut self) -> u64 {
        self.evictions_frozen = true;
        self.dropped.value()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.in_flight.is_none()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn has_queued_events(&self) -> bool {
        !self.queue.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn queued_events(&self) -> usize {
        self.queue.len()
    }

    #[cfg(test)]
    pub(crate) const fn queued_bytes(&self) -> usize {
        self.queue.bytes()
    }

    #[cfg(test)]
    pub(crate) fn set_dropped_for_test(&mut self, value: u64) {
        self.dropped = CheckedCounter::from_value(value);
    }
}

impl From<EventQueueError> for DeliveryDecision {
    fn from(_: EventQueueError) -> Self {
        DeliveryDecision::Terminate(TerminationReason::Accounting)
    }
}
