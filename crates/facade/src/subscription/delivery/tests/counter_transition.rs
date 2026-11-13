use chigiri_runtime::SubscriptionOverflow;

use super::constrained_settings;
use crate::subscription::delivery::counter::MAX_EXACT_JSON_INTEGER;
use crate::subscription::delivery::{
    DeliveryController, DeliveryDecision, DeliveryResources, MonotonicInstant, TerminationReason,
};

fn controller() -> (
    DeliveryController<&'static str>,
    super::super::EndpointByteOwner,
) {
    let settings = constrained_settings(
        |limits| {
            limits.max_queued_events = 3;
            limits.max_queued_bytes = 20;
            limits.max_encoded_event_bytes = 5;
            limits.max_endpoint_encoded_bytes = 20;
        },
        |_| {},
    );
    let resources = DeliveryResources::new(&settings).unwrap();
    let bytes = resources.byte_owner();
    (
        DeliveryController::new(&settings, bytes.clone(), SubscriptionOverflow::DropOldest),
        bytes,
    )
}

fn enqueue(controller: &mut DeliveryController<&'static str>, payload: &'static str, now: u64) {
    let now = MonotonicInstant::from_nanos(now);
    let reservation = controller.reserve_before_poll(now).unwrap();
    controller.enqueue(reservation, 4, payload, now).unwrap();
}

#[test]
fn drop_counter_capacity_is_checked_before_the_fifo_transition() {
    let (mut success, success_bytes) = controller();
    enqueue(&mut success, "first", 0);
    enqueue(&mut success, "second", 1);
    success.set_dropped_for_test(MAX_EXACT_JSON_INTEGER - 1);
    assert_eq!(success.evict_one(), Ok(true));
    assert_eq!(success.dropped_total(), MAX_EXACT_JSON_INTEGER);
    assert_eq!(success.queued_events(), 1);
    assert_eq!(success.queued_bytes(), 4);
    assert_eq!(success_bytes.used(), 4);
    let send = success
        .begin_send_with(MonotonicInstant::from_nanos(2), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    assert_eq!(send.payload(), &"second");

    let (mut exhausted, exhausted_bytes) = controller();
    enqueue(&mut exhausted, "first", 0);
    enqueue(&mut exhausted, "second", 1);
    exhausted.set_dropped_for_test(MAX_EXACT_JSON_INTEGER);
    assert_eq!(
        exhausted.evict_one(),
        Err(DeliveryDecision::Terminate(
            TerminationReason::CounterExhausted
        ))
    );
    assert_eq!(exhausted.dropped_total(), MAX_EXACT_JSON_INTEGER);
    assert_eq!(exhausted.queued_events(), 2);
    assert_eq!(exhausted.queued_bytes(), 8);
    assert_eq!(exhausted_bytes.used(), 8);
    let send = exhausted
        .begin_send_with(MonotonicInstant::from_nanos(2), |payload, _| Ok(payload))
        .unwrap()
        .unwrap();
    assert_eq!(send.payload(), &"first");
}
