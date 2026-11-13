use chigiri_runtime::SubscriptionOverflow;

use crate::subscription::delivery::{
    DeliveryDecision, DeliverySnapshot, EvaluationPoint, TerminationReason, evaluate,
};

const POINTS: [EvaluationPoint; 6] = [
    EvaluationPoint::Admission,
    EvaluationPoint::Reservation,
    EvaluationPoint::Enqueue,
    EvaluationPoint::Dequeue,
    EvaluationPoint::Progress,
    EvaluationPoint::Timer,
];
const POLICIES: [SubscriptionOverflow; 2] = [
    SubscriptionOverflow::Backpressure,
    SubscriptionOverflow::DropOldest,
];
const BYTE_CASES: [(usize, usize, Option<usize>); 9] = [
    (0, 0, None),
    (5, 0, None),
    (6, 0, None),
    (4, 1, None),
    (usize::MAX, 1, None),
    (0, 0, Some(0)),
    (0, 0, Some(5)),
    (0, 0, Some(6)),
    (usize::MAX, 0, Some(1)),
];

#[derive(Clone, Copy)]
struct MatrixCase {
    queued_events: usize,
    queued_bytes: usize,
    in_flight_bytes: usize,
    candidate_bytes: Option<usize>,
    evictable_events: usize,
}

#[test]
fn bounded_matrix_matches_the_closed_normative_or_model() {
    for overflow in POLICIES {
        for point in POINTS {
            for flags in 0_u16..1024 {
                for queued_events in [0, 2, 3, 4] {
                    for (queued_bytes, in_flight_bytes, candidate_bytes) in BYTE_CASES {
                        for evictable_events in [0, 1] {
                            let snapshot = snapshot(
                                overflow,
                                point,
                                flags,
                                MatrixCase {
                                    queued_events,
                                    queued_bytes,
                                    in_flight_bytes,
                                    candidate_bytes,
                                    evictable_events,
                                },
                            );
                            assert_eq!(evaluate(snapshot), expected(snapshot), "{snapshot:?}");
                        }
                    }
                }
            }
        }
    }
}

fn snapshot(
    overflow: SubscriptionOverflow,
    point: EvaluationPoint,
    flags: u16,
    case: MatrixCase,
) -> DeliverySnapshot {
    DeliverySnapshot {
        point,
        overflow,
        invariant_failure: flag(flags, 0),
        accounting_failure: flag(flags, 1),
        counter_exhausted: flag(flags, 2),
        active_limit_reached: flag(flags, 3),
        oldest_queue_expired: flag(flags, 4),
        send_pending: flag(flags, 5),
        send_progress_expired: flag(flags, 6),
        shared_reservation_failed: flag(flags, 7),
        shared_pressure_expired: flag(flags, 8),
        evictions_frozen: flag(flags, 9),
        queued_events: case.queued_events,
        queued_bytes: case.queued_bytes,
        in_flight_bytes: case.in_flight_bytes,
        candidate_bytes: case.candidate_bytes,
        max_queued_events: 3,
        max_queued_bytes: 5,
        max_event_bytes: 5,
        evictable_events: case.evictable_events,
    }
}

const fn flag(flags: u16, bit: u32) -> bool {
    flags & (1_u16 << bit) != 0
}

fn expected(snapshot: DeliverySnapshot) -> DeliveryDecision {
    if snapshot.invariant_failure {
        return DeliveryDecision::Terminate(TerminationReason::Invariant);
    }
    let total_bytes = snapshot
        .queued_bytes
        .checked_add(snapshot.in_flight_bytes)
        .and_then(|bytes| bytes.checked_add(snapshot.candidate_bytes.unwrap_or(0)));
    if snapshot.accounting_failure || total_bytes.is_none() {
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
    let local_pressure = (snapshot.candidate_bytes.is_some()
        && snapshot.queued_events >= snapshot.max_queued_events)
        || total_bytes.is_some_and(|bytes| bytes > snapshot.max_queued_bytes)
        || snapshot.oldest_queue_expired
        || snapshot.shared_reservation_failed;
    if local_pressure
        && !snapshot.evictions_frozen
        && snapshot.overflow == SubscriptionOverflow::DropOldest
        && snapshot.evictable_events > 0
    {
        let unavailable_until_physical_release =
            snapshot.candidate_bytes.is_some_and(|candidate| {
                snapshot
                    .in_flight_bytes
                    .checked_add(candidate)
                    .is_some_and(|sum| sum > snapshot.max_queued_bytes)
            });
        if unavailable_until_physical_release
            && !snapshot.oldest_queue_expired
            && !snapshot.shared_reservation_failed
        {
            return DeliveryDecision::Pause;
        }
        return DeliveryDecision::EvictWhole(1);
    }
    if local_pressure {
        DeliveryDecision::Pause
    } else {
        DeliveryDecision::Admit
    }
}
