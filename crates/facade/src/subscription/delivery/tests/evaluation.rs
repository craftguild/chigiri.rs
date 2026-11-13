use std::time::Duration;

use chigiri_runtime::SubscriptionOverflow;

use super::super::{
    CheckedCounter, DeliveryDecision, DeliverySnapshot, EvaluationPoint, MonotonicInstant,
    SharedPressureWait, TerminationReason, evaluate,
};
use crate::subscription::delivery::counter::MAX_EXACT_JSON_INTEGER;

fn snapshot() -> DeliverySnapshot {
    DeliverySnapshot {
        point: EvaluationPoint::Enqueue,
        overflow: SubscriptionOverflow::Backpressure,
        invariant_failure: false,
        accounting_failure: false,
        counter_exhausted: false,
        active_limit_reached: false,
        queued_events: 0,
        queued_bytes: 0,
        in_flight_bytes: 0,
        candidate_bytes: None,
        max_queued_events: 2,
        max_queued_bytes: 10,
        max_event_bytes: 8,
        oldest_queue_expired: false,
        send_pending: false,
        send_progress_expired: false,
        shared_reservation_failed: false,
        shared_pressure_expired: false,
        evictable_events: 0,
        evictions_frozen: false,
    }
}

#[test]
fn unrecoverable_reasons_and_rejection_have_fixed_precedence() {
    let mut state = snapshot();
    state.invariant_failure = true;
    state.accounting_failure = true;
    state.counter_exhausted = true;
    state.active_limit_reached = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::Invariant)
    );

    state.invariant_failure = false;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::Accounting)
    );
    state.accounting_failure = false;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::CounterExhausted)
    );
    state.counter_exhausted = false;
    state.candidate_bytes = Some(9);
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::OversizedEvent)
    );
    state.candidate_bytes = None;
    state.send_pending = true;
    state.send_progress_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::SendStalled)
    );
    state.send_pending = false;
    state.send_progress_expired = false;
    state.shared_pressure_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::SharedBudgetPressureExpired)
    );
}

#[test]
fn backpressure_pauses_all_capacity_pressure_and_expires_queue_age() {
    for point in [
        EvaluationPoint::Admission,
        EvaluationPoint::Reservation,
        EvaluationPoint::Enqueue,
        EvaluationPoint::Dequeue,
        EvaluationPoint::Progress,
        EvaluationPoint::Timer,
    ] {
        let mut state = snapshot();
        state.point = point;
        state.queued_events = 2;
        state.candidate_bytes = Some(1);
        assert_eq!(evaluate(state), DeliveryDecision::Pause);
    }

    let mut bytes = snapshot();
    bytes.in_flight_bytes = 8;
    bytes.candidate_bytes = Some(3);
    assert_eq!(evaluate(bytes), DeliveryDecision::Pause);

    let mut shared = snapshot();
    shared.shared_reservation_failed = true;
    assert_eq!(evaluate(shared), DeliveryDecision::Pause);

    let mut aged = snapshot();
    aged.oldest_queue_expired = true;
    aged.evictable_events = 1;
    assert_eq!(
        evaluate(aged),
        DeliveryDecision::Terminate(TerminationReason::QueueAgeExpired)
    );
}

#[test]
fn drop_oldest_evicts_one_then_requires_a_complete_recheck() {
    let mut state = snapshot();
    state.overflow = SubscriptionOverflow::DropOldest;
    state.queued_events = 2;
    state.queued_bytes = 9;
    state.candidate_bytes = Some(2);
    state.evictable_events = 2;
    assert_eq!(evaluate(state), DeliveryDecision::EvictWhole(1));

    state.queued_events = 1;
    state.queued_bytes = 5;
    state.evictable_events = 1;
    assert_eq!(evaluate(state), DeliveryDecision::Admit);

    state.shared_reservation_failed = true;
    assert_eq!(evaluate(state), DeliveryDecision::EvictWhole(1));
    state.evictable_events = 0;
    assert_eq!(evaluate(state), DeliveryDecision::Pause);
}

#[test]
fn irreducible_physical_pressure_preserves_survivors_but_not_expired_entries() {
    let mut state = snapshot();
    state.overflow = SubscriptionOverflow::DropOldest;
    state.queued_events = 1;
    state.queued_bytes = 3;
    state.in_flight_bytes = 6;
    state.candidate_bytes = Some(6);
    state.evictable_events = 1;
    assert_eq!(evaluate(state), DeliveryDecision::Pause);
    state.oldest_queue_expired = true;
    assert_eq!(evaluate(state), DeliveryDecision::EvictWhole(1));
    state.oldest_queue_expired = false;
    state.shared_reservation_failed = true;
    assert_eq!(evaluate(state), DeliveryDecision::EvictWhole(1));
    state.send_pending = true;
    state.send_progress_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::SendStalled)
    );
}

#[test]
fn frozen_terminal_counter_blocks_eviction_but_preserves_deadline_precedence() {
    let mut state = snapshot();
    state.overflow = SubscriptionOverflow::DropOldest;
    state.evictions_frozen = true;
    state.queued_events = 2;
    state.candidate_bytes = Some(1);
    state.evictable_events = 2;
    assert_eq!(evaluate(state), DeliveryDecision::Pause);

    state.oldest_queue_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::QueueAgeExpired)
    );

    state.send_pending = true;
    state.send_progress_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::SendStalled)
    );

    state.shared_pressure_expired = true;
    assert_eq!(
        evaluate(state),
        DeliveryDecision::Terminate(TerminationReason::SendStalled)
    );
}

#[test]
fn checked_arithmetic_and_exact_counters_fail_without_mutation() {
    let mut overflow = snapshot();
    overflow.queued_bytes = usize::MAX;
    overflow.in_flight_bytes = 1;
    assert_eq!(
        evaluate(overflow),
        DeliveryDecision::Terminate(TerminationReason::Accounting)
    );

    let mut counter = CheckedCounter::from_value(MAX_EXACT_JSON_INTEGER - 1);
    assert_eq!(counter.increment(), Ok(MAX_EXACT_JSON_INTEGER));
    assert!(counter.increment().is_err());
    assert_eq!(counter.value(), MAX_EXACT_JSON_INTEGER);
}

#[test]
fn pressure_wait_is_armed_only_by_failure_and_never_extended() {
    let start = MonotonicInstant::from_nanos(10);
    let duration = Duration::from_nanos(5);
    let mut wait = SharedPressureWait::new();
    assert!(!wait.is_armed());
    assert!(!wait.is_expired(MonotonicInstant::from_nanos(100)));

    wait.note_failure(start, duration).unwrap();
    let first_deadline = wait.deadline().unwrap();
    wait.note_failure(MonotonicInstant::from_nanos(12), duration)
        .unwrap();
    assert_eq!(wait.deadline(), Some(first_deadline));
    assert!(!wait.is_expired(MonotonicInstant::from_nanos(14)));
    assert!(wait.is_expired(MonotonicInstant::from_nanos(15)));

    wait.note_success();
    assert!(!wait.is_armed());
}
