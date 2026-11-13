use std::time::Duration;

use chigiri_runtime::SubscriptionOverflow;

use super::constrained_settings;
use crate::subscription::delivery::{
    DeliveryController, DeliveryDecision, DeliveryResources, MonotonicInstant, TerminationReason,
};

fn resources() -> (super::super::EndpointByteOwner, DeliveryResources) {
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 5;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
            limits.max_queue_age = Duration::from_nanos(5);
            limits.send_no_progress_timeout = Duration::from_nanos(5);
        },
        |_| {},
    );
    let resources = DeliveryResources::new(&settings).unwrap();
    (resources.byte_owner(), resources)
}

fn enqueue(controller: &mut DeliveryController<&'static str>, payload: &'static str, now: u64) {
    let now = MonotonicInstant::from_nanos(now);
    let reservation = controller.reserve_before_poll(now).unwrap();
    controller.enqueue(reservation, 4, payload, now).unwrap();
}

#[test]
fn second_begin_send_cannot_hide_or_dequeue_the_owned_slot() {
    let (bytes, resources) = resources();
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 20;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
        },
        |_| {},
    );
    let mut controller =
        DeliveryController::new(&settings, bytes.clone(), SubscriptionOverflow::Backpressure);
    enqueue(&mut controller, "first", 0);
    enqueue(&mut controller, "second", 1);

    assert_eq!(
        controller.evaluate_dequeue(MonotonicInstant::from_nanos(2)),
        DeliveryDecision::Admit
    );
    let send = controller
        .begin_send_with(MonotonicInstant::from_nanos(2), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    assert_eq!(send.payload(), &"first");
    assert_eq!(controller.queued_events(), 1);
    assert_eq!(controller.queued_bytes(), 4);
    assert_eq!(resources.byte_owner().used(), 8);
    assert!(matches!(
        controller.begin_send_with(MonotonicInstant::from_nanos(3), |payload, _| Ok(payload)),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    ));
    assert_eq!(controller.queued_events(), 1);
    assert_eq!(controller.queued_bytes(), 4);
    assert_eq!(resources.byte_owner().used(), 8);
}

#[test]
fn progress_is_one_shot_and_finish_requires_exact_sender_release() {
    let (bytes, resources) = resources();
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 5;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
            limits.send_no_progress_timeout = Duration::from_nanos(5);
        },
        |_| {},
    );
    let mut controller =
        DeliveryController::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    enqueue(&mut controller, "event", 0);
    let mut send = controller
        .begin_send_with(MonotonicInstant::from_nanos(10), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    let mut progress = send.take_progress().unwrap();
    assert!(send.take_progress().is_none());
    assert_eq!(
        progress.accept(4, MonotonicInstant::from_nanos(11)),
        Ok(true)
    );
    assert_eq!(controller.next_deadline(), Ok(None));
    assert_eq!(
        controller.evaluate_progress(MonotonicInstant::from_nanos(100)),
        DeliveryDecision::Admit
    );
    assert_eq!(resources.byte_owner().used(), 4);
    assert!(matches!(
        controller.reserve_before_poll(MonotonicInstant::from_nanos(12)),
        Err(DeliveryDecision::Pause)
    ));

    drop(send);
    assert_eq!(controller.finish_send(progress), Ok(()));
    assert_eq!(resources.byte_owner().used(), 0);
}

#[test]
fn a_live_or_foreign_sender_cannot_clear_an_inflight_slot() {
    let (bytes, resources) = resources();
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 20;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
        },
        |_| {},
    );
    let mut first =
        DeliveryController::new(&settings, bytes.clone(), SubscriptionOverflow::Backpressure);
    let mut second = DeliveryController::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    enqueue(&mut first, "first", 0);
    enqueue(&mut second, "second", 0);
    let mut first_send = first
        .begin_send_with(MonotonicInstant::from_nanos(1), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    let first_progress = first_send.take_progress().unwrap();
    let second_send = second
        .begin_send_with(MonotonicInstant::from_nanos(1), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();

    assert_eq!(
        second.finish_send(first_progress),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    );
    assert_eq!(resources.byte_owner().used(), 8);
    drop((first_send, second_send, first, second));
    assert_eq!(resources.byte_owner().used(), 0);
}

#[test]
fn finish_before_sender_drop_fails_closed_without_releasing_charge() {
    let (bytes, resources) = resources();
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 20;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
        },
        |_| {},
    );
    let mut controller =
        DeliveryController::new(&settings, bytes, SubscriptionOverflow::Backpressure);
    enqueue(&mut controller, "event", 0);
    let mut send = controller
        .begin_send_with(MonotonicInstant::from_nanos(1), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    let mut progress = send.take_progress().unwrap();
    progress.accept(4, MonotonicInstant::from_nanos(2)).unwrap();
    assert_eq!(
        controller.finish_send(progress),
        Err(DeliveryDecision::Terminate(TerminationReason::Invariant))
    );
    assert_eq!(resources.byte_owner().used(), 4);
    drop(send);
    assert_eq!(resources.byte_owner().used(), 4);
    drop(controller);
    assert_eq!(resources.byte_owner().used(), 0);
}
