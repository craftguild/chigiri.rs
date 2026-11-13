//! Pure evaluation of all subscription delivery limits with OR semantics.

use chigiri_runtime::SubscriptionOverflow;

/// The transition that requested a complete policy re-evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvaluationPoint {
    Admission,
    Reservation,
    Enqueue,
    Dequeue,
    Progress,
    Timer,
}

/// Immutable input to the common delivery policy evaluator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DeliverySnapshot {
    pub(crate) point: EvaluationPoint,
    pub(crate) overflow: SubscriptionOverflow,
    pub(crate) invariant_failure: bool,
    pub(crate) accounting_failure: bool,
    pub(crate) counter_exhausted: bool,
    pub(crate) active_limit_reached: bool,
    pub(crate) queued_events: usize,
    pub(crate) queued_bytes: usize,
    pub(crate) in_flight_bytes: usize,
    pub(crate) candidate_bytes: Option<usize>,
    pub(crate) max_queued_events: usize,
    pub(crate) max_queued_bytes: usize,
    pub(crate) max_event_bytes: usize,
    pub(crate) oldest_queue_expired: bool,
    pub(crate) send_pending: bool,
    pub(crate) send_progress_expired: bool,
    pub(crate) shared_reservation_failed: bool,
    pub(crate) shared_pressure_expired: bool,
    pub(crate) evictable_events: usize,
    pub(crate) evictions_frozen: bool,
}

/// The only actions exposed to protocol delivery owners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeliveryDecision {
    Admit,
    Pause,
    EvictWhole(usize),
    RejectNew,
    Terminate(TerminationReason),
}

/// Closed terminal reasons retained for safe observation and bridge mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminationReason {
    Invariant,
    Accounting,
    CounterExhausted,
    OversizedEvent,
    QueueAgeExpired,
    SendStalled,
    SharedBudgetPressureExpired,
}

/// Evaluates every applicable condition before selecting one closed action.
pub(crate) fn evaluate(snapshot: DeliverySnapshot) -> DeliveryDecision {
    if snapshot.invariant_failure {
        return DeliveryDecision::Terminate(TerminationReason::Invariant);
    }
    if snapshot.accounting_failure || queue_bytes(&snapshot).is_none() {
        return DeliveryDecision::Terminate(TerminationReason::Accounting);
    }
    if snapshot.counter_exhausted {
        return DeliveryDecision::Terminate(TerminationReason::CounterExhausted);
    }
    if snapshot
        .candidate_bytes
        .is_some_and(|bytes| bytes > snapshot.max_event_bytes)
    {
        return DeliveryDecision::Terminate(TerminationReason::OversizedEvent);
    }
    if snapshot.send_pending && snapshot.send_progress_expired {
        return DeliveryDecision::Terminate(TerminationReason::SendStalled);
    }
    if snapshot.shared_pressure_expired {
        return DeliveryDecision::Terminate(TerminationReason::SharedBudgetPressureExpired);
    }
    if snapshot.oldest_queue_expired
        && (snapshot.overflow == SubscriptionOverflow::Backpressure || snapshot.evictions_frozen)
    {
        return DeliveryDecision::Terminate(TerminationReason::QueueAgeExpired);
    }
    if snapshot.active_limit_reached {
        return DeliveryDecision::RejectNew;
    }

    let count_pressure =
        snapshot.candidate_bytes.is_some() && snapshot.queued_events >= snapshot.max_queued_events;
    let byte_pressure =
        queue_bytes(&snapshot).is_some_and(|bytes| bytes > snapshot.max_queued_bytes);
    let recoverable = count_pressure
        || byte_pressure
        || snapshot.oldest_queue_expired
        || snapshot.shared_reservation_failed;

    if recoverable
        && !snapshot.evictions_frozen
        && snapshot.overflow == SubscriptionOverflow::DropOldest
        && snapshot.evictable_events > 0
    {
        // Capacity eviction cannot admit a candidate while the physical event
        // alone leaves too little room. Preserve live queued survivors until
        // that owner drops. Expiry and real shared pressure remain independent.
        let irreducible = snapshot.candidate_bytes.is_some_and(|candidate| {
            snapshot
                .in_flight_bytes
                .checked_add(candidate)
                .is_some_and(|bytes| bytes > snapshot.max_queued_bytes)
        });
        if irreducible && !snapshot.oldest_queue_expired && !snapshot.shared_reservation_failed {
            return DeliveryDecision::Pause;
        }
        return DeliveryDecision::EvictWhole(1);
    }
    if recoverable {
        return DeliveryDecision::Pause;
    }
    DeliveryDecision::Admit
}

fn queue_bytes(snapshot: &DeliverySnapshot) -> Option<usize> {
    snapshot
        .queued_bytes
        .checked_add(snapshot.in_flight_bytes)?
        .checked_add(snapshot.candidate_bytes.unwrap_or(0))
}
