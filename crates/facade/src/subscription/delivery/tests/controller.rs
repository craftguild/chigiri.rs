use std::time::Duration;

use chigiri_runtime::SubscriptionOverflow;

use super::constrained_settings;
use crate::subscription::delivery::{
    DeliveryController, DeliveryDecision, DeliveryResources, EvaluationPoint, MonotonicInstant,
    TerminationReason,
};

fn controller_settings() -> crate::subscription::settings::ValidatedSubscriptionSettings {
    constrained_settings(
        |limits| {
            limits.max_queued_events = 1;
            limits.max_queued_bytes = 10;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 10;
            limits.max_queue_age = Duration::from_nanos(5);
            limits.send_no_progress_timeout = Duration::from_nanos(5);
        },
        |_| {},
    )
}

#[test]
fn actual_shared_pressure_has_finite_wait_but_idle_does_not() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let bytes = resources.byte_owner();
    let external = bytes.try_reserve(10).unwrap();
    let mut controller =
        DeliveryController::<()>::new(&settings, bytes.clone(), SubscriptionOverflow::Backpressure);

    assert!(matches!(
        controller.reserve_before_poll(MonotonicInstant::from_nanos(10)),
        Err(DeliveryDecision::Pause)
    ));
    assert_eq!(
        controller.next_deadline(),
        Ok(Some(MonotonicInstant::from_nanos(15)))
    );
    assert_eq!(
        controller.decision(
            EvaluationPoint::Timer,
            MonotonicInstant::from_nanos(15),
            false,
            None,
        ),
        DeliveryDecision::Terminate(TerminationReason::SharedBudgetPressureExpired)
    );
    drop(external);
    assert!(matches!(
        controller.reserve_before_poll(MonotonicInstant::from_nanos(15)),
        Err(DeliveryDecision::Terminate(
            TerminationReason::SharedBudgetPressureExpired
        ))
    ));

    let idle = DeliveryController::<()>::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    assert_eq!(
        idle.decision(
            EvaluationPoint::Timer,
            MonotonicInstant::from_nanos(u64::MAX),
            false,
            None,
        ),
        DeliveryDecision::Admit
    );
}

#[test]
fn reservation_success_before_expiry_is_the_only_pressure_reset() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let bytes = resources.byte_owner();
    let external = bytes.try_reserve(10).unwrap();
    let mut controller =
        DeliveryController::<()>::new(&settings, bytes.clone(), SubscriptionOverflow::Backpressure);
    assert!(
        controller
            .reserve_before_poll(MonotonicInstant::from_nanos(10))
            .is_err()
    );
    drop(external);

    let reservation = controller
        .reserve_before_poll(MonotonicInstant::from_nanos(14))
        .unwrap();
    drop(reservation);
    assert_eq!(
        controller.decision(
            EvaluationPoint::Timer,
            MonotonicInstant::from_nanos(100),
            false,
            None,
        ),
        DeliveryDecision::Admit
    );
}

#[test]
fn drop_oldest_evicts_fifo_and_updates_the_latest_counter() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let mut controller = DeliveryController::<&str>::new(
        &settings,
        resources.byte_owner(),
        SubscriptionOverflow::DropOldest,
    );
    let start = MonotonicInstant::from_nanos(0);
    let reservation = controller.reserve_before_poll(start).unwrap();
    assert_eq!(
        controller.evaluate_candidate(start, 4),
        DeliveryDecision::Admit
    );
    controller.enqueue(reservation, 4, "first", start).unwrap();

    let reservation = controller
        .reserve_before_poll(MonotonicInstant::from_nanos(1))
        .unwrap();
    assert_eq!(controller.dropped_total(), 0);
    assert_eq!(
        controller.evaluate_candidate(MonotonicInstant::from_nanos(1), 4),
        DeliveryDecision::EvictWhole(1)
    );
    assert_eq!(controller.evict_one(), Ok(true));
    assert_eq!(controller.dropped_total(), 1);
    controller
        .enqueue(reservation, 4, "second", MonotonicInstant::from_nanos(1))
        .unwrap();
    assert_eq!(controller.queued_events(), 1);
    assert_eq!(controller.queued_bytes(), 4);
    let in_flight = controller
        .begin_send_with(MonotonicInstant::from_nanos(2), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    assert_eq!(in_flight.payload(), &"second");
    assert_eq!(controller.queued_events(), 0);
}

#[test]
fn frozen_counter_makes_direct_eviction_fail_without_mutation() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let mut controller = DeliveryController::<&str>::new(
        &settings,
        resources.byte_owner(),
        SubscriptionOverflow::DropOldest,
    );
    let now = MonotonicInstant::from_nanos(0);
    let reservation = controller.reserve_before_poll(now).unwrap();
    controller.enqueue(reservation, 4, "kept", now).unwrap();

    assert_eq!(controller.freeze_evictions(), 0);
    assert_eq!(controller.freeze_evictions(), 0);
    assert_eq!(
        controller.evaluate_candidate(MonotonicInstant::from_nanos(1), 4),
        DeliveryDecision::Pause
    );
    assert_eq!(
        controller.evict_one(),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    );
    assert_eq!(controller.dropped_total(), 0);
    assert_eq!(controller.queued_events(), 1);
}

#[test]
fn backpressure_queue_age_and_send_progress_expire_without_new_events() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let mut controller = DeliveryController::new(
        &settings,
        resources.byte_owner(),
        SubscriptionOverflow::Backpressure,
    );
    let start = MonotonicInstant::from_nanos(10);
    let reservation = controller.reserve_before_poll(start).unwrap();
    controller.enqueue(reservation, 4, (), start).unwrap();
    assert_eq!(
        controller.decision(
            EvaluationPoint::Timer,
            MonotonicInstant::from_nanos(15),
            false,
            None,
        ),
        DeliveryDecision::Terminate(TerminationReason::QueueAgeExpired)
    );

    let mut in_flight = controller
        .begin_send_with(MonotonicInstant::from_nanos(20), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    let mut progress = in_flight.take_progress().unwrap();
    progress
        .accept(0, MonotonicInstant::from_nanos(24))
        .unwrap();
    assert_eq!(
        controller.decision(
            EvaluationPoint::Timer,
            MonotonicInstant::from_nanos(25),
            false,
            None,
        ),
        DeliveryDecision::Terminate(TerminationReason::SendStalled)
    );
}

#[test]
fn a_reservation_cannot_cross_controller_ownership() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let bytes = resources.byte_owner();
    let mut first =
        DeliveryController::<()>::new(&settings, bytes.clone(), SubscriptionOverflow::Backpressure);
    let mut second =
        DeliveryController::<()>::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    let now = MonotonicInstant::from_nanos(0);
    let foreign = second.reserve_before_poll(now).unwrap();

    assert_eq!(
        first.enqueue(foreign, 1, (), now),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    );
    assert_eq!(resources.byte_owner().used(), 0);
    assert_eq!(
        first.decision(EvaluationPoint::Enqueue, now, false, None),
        DeliveryDecision::Terminate(TerminationReason::Accounting)
    );
}

#[test]
fn next_deadline_is_idle_safe_and_selects_the_earliest_phase() {
    let settings = controller_settings();
    let resources = DeliveryResources::new(&settings).unwrap();
    let bytes = resources.byte_owner();
    let mut controller =
        DeliveryController::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    assert_eq!(controller.next_deadline(), Ok(None));

    let admitted = MonotonicInstant::from_nanos(10);
    let reservation = controller.reserve_before_poll(admitted).unwrap();
    controller.enqueue(reservation, 4, (), admitted).unwrap();
    assert_eq!(
        controller.next_deadline(),
        Ok(Some(MonotonicInstant::from_nanos(15)))
    );

    let _in_flight = controller
        .begin_send_with(MonotonicInstant::from_nanos(20), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    assert_eq!(
        controller.next_deadline(),
        Ok(Some(MonotonicInstant::from_nanos(25)))
    );

    let mut overflow = DeliveryController::<()>::new(
        &settings,
        resources.byte_owner(),
        SubscriptionOverflow::Backpressure,
    );
    let late = MonotonicInstant::from_nanos(u64::MAX - 2);
    let reservation = overflow.reserve_before_poll(late).unwrap();
    overflow.enqueue(reservation, 1, (), late).unwrap();
    assert_eq!(
        overflow.next_deadline(),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    );
}
