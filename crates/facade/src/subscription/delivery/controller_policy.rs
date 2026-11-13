//! Snapshot construction and deadline policy for a delivery controller.

use std::time::Duration;

use super::{
    DeliveryController, DeliveryDecision, DeliverySnapshot, EvaluationPoint, InFlightSlot,
    MonotonicInstant, TerminationReason, evaluate,
};

#[derive(Clone, Copy)]
pub(super) struct ControllerLimits {
    pub(super) max_queued_events: usize,
    pub(super) max_queued_bytes: usize,
    pub(super) max_event_bytes: usize,
    pub(super) max_queue_age: Duration,
    pub(super) send_no_progress: Duration,
}

impl<Queued, Finalized> DeliveryController<Queued, Finalized> {
    /// Evaluates operation admission against the complete current snapshot.
    pub(crate) fn evaluate_admission(
        &self,
        now: MonotonicInstant,
        active_limit_reached: bool,
    ) -> DeliveryDecision {
        self.evaluate_snapshot(
            EvaluationPoint::Admission,
            now,
            active_limit_reached,
            None,
            true,
        )
    }

    /// Evaluates every armed deadline when the external timer fires.
    pub(crate) fn evaluate_timer(&self, now: MonotonicInstant) -> DeliveryDecision {
        self.evaluate_snapshot(EvaluationPoint::Timer, now, false, None, true)
    }

    pub(crate) fn decision(
        &self,
        point: EvaluationPoint,
        now: MonotonicInstant,
        active_limit_reached: bool,
        candidate_bytes: Option<usize>,
    ) -> DeliveryDecision {
        self.evaluate_snapshot(point, now, active_limit_reached, candidate_bytes, true)
    }

    /// Evaluates dequeue without applying source-side shared reservation pressure.
    pub(crate) fn evaluate_dequeue(&self, now: MonotonicInstant) -> DeliveryDecision {
        self.decision_without_pressure(EvaluationPoint::Dequeue, now, false, None)
    }

    /// Evaluates accepted-byte progress without source reservation pressure.
    pub(crate) fn evaluate_progress(&self, now: MonotonicInstant) -> DeliveryDecision {
        self.decision_without_pressure(EvaluationPoint::Progress, now, false, None)
    }

    /// Returns the earliest policy deadline without installing a timer.
    pub(crate) fn next_deadline(&self) -> Result<Option<MonotonicInstant>, DeliveryDecision> {
        let queue = self.queue_deadline()?;
        let send = self.send_deadline()?;
        let pressure = self
            .pressure
            .deadline()
            .map(|deadline| deadline.expires_at());
        Ok([queue, send, pressure].into_iter().flatten().min())
    }

    fn queue_deadline(&self) -> Result<Option<MonotonicInstant>, DeliveryDecision> {
        self.queue
            .oldest_admitted_at()
            .map(|started| started.checked_add(self.limits.max_queue_age))
            .transpose()
            .map_err(|_| DeliveryDecision::Terminate(TerminationReason::Invariant))
    }

    fn send_deadline(&self) -> Result<Option<MonotonicInstant>, DeliveryDecision> {
        let Some(event) = self.in_flight.as_ref() else {
            return Ok(None);
        };
        let (accepted, last_progress) = event
            .progress()
            .map_err(|_| DeliveryDecision::Terminate(TerminationReason::Accounting))?;
        if accepted == event.bytes() {
            return Ok(None);
        }
        last_progress
            .checked_add(self.limits.send_no_progress)
            .map(Some)
            .map_err(|_| DeliveryDecision::Terminate(TerminationReason::Invariant))
    }

    pub(super) fn decision_without_pressure(
        &self,
        point: EvaluationPoint,
        now: MonotonicInstant,
        active_limit_reached: bool,
        candidate_bytes: Option<usize>,
    ) -> DeliveryDecision {
        self.evaluate_snapshot(point, now, active_limit_reached, candidate_bytes, false)
    }

    fn evaluate_snapshot(
        &self,
        point: EvaluationPoint,
        now: MonotonicInstant,
        active_limit_reached: bool,
        candidate_bytes: Option<usize>,
        include_pressure: bool,
    ) -> DeliveryDecision {
        let oldest_queue_expired = self.queue.oldest_admitted_at().is_some_and(|started| {
            started
                .checked_add(self.limits.max_queue_age)
                .is_ok_and(|deadline| now >= deadline)
        });
        let in_flight = self
            .in_flight
            .as_ref()
            .map(InFlightSlot::progress)
            .transpose();
        let in_flight_bytes = self.in_flight.as_ref().map_or(0, InFlightSlot::bytes);
        let send_pending = in_flight.as_ref().is_ok_and(|state| {
            state
                .as_ref()
                .is_some_and(|(accepted, _)| *accepted < in_flight_bytes)
        });
        let send_progress_expired = send_pending
            && in_flight.as_ref().is_ok_and(|state| {
                state.as_ref().is_some_and(|(_, last_progress)| {
                    last_progress
                        .checked_add(self.limits.send_no_progress)
                        .is_ok_and(|deadline| now >= deadline)
                })
            });
        let timer_failure = self
            .queue
            .oldest_admitted_at()
            .is_some_and(|started| started.checked_add(self.limits.max_queue_age).is_err())
            || (send_pending
                && in_flight.as_ref().is_ok_and(|state| {
                    state.as_ref().is_some_and(|(_, last_progress)| {
                        last_progress
                            .checked_add(self.limits.send_no_progress)
                            .is_err()
                    })
                }));
        evaluate(DeliverySnapshot {
            point,
            overflow: self.overflow,
            invariant_failure: timer_failure,
            accounting_failure: self.accounting_failure
                || self.bytes.is_corrupted()
                || in_flight.is_err(),
            counter_exhausted: self.counter_exhausted,
            active_limit_reached,
            queued_events: self.queue.len(),
            queued_bytes: self.queue.bytes(),
            in_flight_bytes,
            candidate_bytes,
            max_queued_events: self.limits.max_queued_events,
            max_queued_bytes: self.limits.max_queued_bytes,
            max_event_bytes: self.limits.max_event_bytes,
            oldest_queue_expired,
            send_pending,
            send_progress_expired,
            shared_reservation_failed: include_pressure && self.pressure.is_armed(),
            shared_pressure_expired: include_pressure && self.pressure.is_expired(now),
            evictable_events: self.queue.len(),
            evictions_frozen: self.evictions_frozen,
        })
    }
}
