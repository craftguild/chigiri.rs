//! Real bytes blocked only by a non-evictable physical event retain one candidate.

use chigiri_http::{__encode_subscription_event, SubscriptionTransport};
use chigiri_runtime::SubscriptionOverflow;

use super::super::{SubscriptionOutbox, SubscriptionOutboxSignal};
use super::fixture::{EVENT_LIMIT, counter, enqueue, outbox, response, settings};
use crate::subscription::delivery::EndpointByteOwner;

fn constrained() -> (SubscriptionOutbox, EndpointByteOwner) {
    let baseline = settings(2, 100, 10);
    let mut limits = *baseline.limits();
    limits.max_queued_bytes = EVENT_LIMIT;
    let settings = crate::subscription::settings::ValidatedSubscriptionSettings::new(
        limits,
        *baseline.protocol(),
    );
    let bytes = EndpointByteOwner::new(4 * EVENT_LIMIT);
    (
        outbox(
            &settings,
            bytes.clone(),
            SubscriptionOverflow::DropOldest,
            SubscriptionTransport::ServerSentEvents,
        ),
        bytes,
    )
}

fn actual(value: &str) -> usize {
    __encode_subscription_event(
        SubscriptionTransport::ServerSentEvents,
        None,
        &response(value),
        EVENT_LIMIT,
    )
    .unwrap()
    .len()
}

#[test]
fn waiting_candidate_shrinks_once_stops_demand_and_moves_after_last_physical_drop() {
    let (mut outbox, bytes) = constrained();
    let first = "a".repeat(600);
    let second = "b".repeat(600);
    enqueue(&mut outbox, &first, 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    progress.accept(frame.len(), 1).unwrap();
    let reservation = outbox.reserve_before_poll(2).unwrap();
    assert_eq!(bytes.used(), frame.len() + EVENT_LIMIT);
    outbox
        .encode_and_enqueue(reservation, &response(&second), 2)
        .unwrap();
    assert!(frame.len() + actual(&second) > EVENT_LIMIT);
    assert_eq!(bytes.used(), frame.len() + actual(&second));
    assert_eq!(
        outbox.reserve_before_poll(3).err(),
        Some(SubscriptionOutboxSignal::Wait)
    );
    assert_eq!(bytes.used(), frame.len() + actual(&second));

    // Exact shrink frees capacity for an unrelated subscription immediately.
    let other = bytes.try_reserve(bytes.limit() - bytes.used()).unwrap();
    drop(other);
    outbox.on_timer(50).unwrap();
    drop(frame);
    outbox.finish_send(progress, 50).unwrap();
    assert_eq!(bytes.used(), actual(&second));
    let (frame, mut progress) = outbox.begin_send(50).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_ref())
            .unwrap()
            .contains(&second)
    );
    assert_eq!(counter(frame.as_ref()), Some(0));
    progress.accept(frame.len(), 50).unwrap();
    drop(frame);
    outbox.finish_send(progress, 50).unwrap();
    assert_eq!(bytes.used(), 0);
}

#[test]
fn waiting_candidate_cannot_hide_the_original_send_stall() {
    let (mut outbox, bytes) = constrained();
    enqueue(&mut outbox, &"a".repeat(600), 0).unwrap();
    let (frame, _progress) = outbox.begin_send(0).unwrap().unwrap();
    let reservation = outbox.reserve_before_poll(1).unwrap();
    outbox
        .encode_and_enqueue(reservation, &response(&"b".repeat(600)), 1)
        .unwrap();
    assert!(bytes.used() > frame.len());
    assert_eq!(
        outbox.on_timer(10),
        Err(SubscriptionOutboxSignal::SendStalled)
    );
    assert_eq!(bytes.used(), frame.len());
    drop(frame);
    assert_eq!(bytes.used(), 0);
    assert!(!bytes.is_corrupted());
}

#[test]
fn cancellation_discards_only_the_candidate_and_keeps_physical_bytes_charged() {
    let (mut outbox, bytes) = constrained();
    enqueue(&mut outbox, &"a".repeat(600), 0).unwrap();
    let (frame, _) = outbox.begin_send(0).unwrap().unwrap();
    let reservation = outbox.reserve_before_poll(1).unwrap();
    outbox
        .encode_and_enqueue(reservation, &response(&"b".repeat(600)), 1)
        .unwrap();
    outbox.cancel();
    assert_eq!(bytes.used(), frame.len());
    assert_eq!(
        outbox.reserve_before_poll(2).err(),
        Some(SubscriptionOutboxSignal::Cancelled)
    );
    drop(frame);
    assert_eq!(bytes.used(), 0);
    assert!(!bytes.is_corrupted());
}

#[test]
fn irreducible_inflight_pressure_does_not_evict_an_unexpired_survivor() {
    let (mut outbox, bytes) = constrained();
    enqueue(&mut outbox, &"a".repeat(600), 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    progress.accept(frame.len(), 1).unwrap();
    enqueue(&mut outbox, "kept", 2).unwrap();
    let before = bytes.used();
    let second = "b".repeat(600);
    let reservation = outbox.reserve_before_poll(3).unwrap();
    outbox
        .encode_and_enqueue(reservation, &response(&second), 3)
        .unwrap();
    assert_eq!(bytes.used(), before + actual(&second));
    assert_eq!(
        outbox.reserve_before_poll(4).err(),
        Some(SubscriptionOutboxSignal::Wait)
    );
    drop(frame);
    outbox.finish_send(progress, 5).unwrap();
    let (frame, mut progress) = outbox.begin_send(5).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_ref())
            .unwrap()
            .contains("kept")
    );
    assert_eq!(counter(frame.as_ref()), Some(0));
    progress.accept(frame.len(), 5).unwrap();
    drop(frame);
    outbox.finish_send(progress, 5).unwrap();
    let (frame, mut progress) = outbox.begin_send(6).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_ref())
            .unwrap()
            .contains(&second)
    );
    assert_eq!(counter(frame.as_ref()), Some(0));
    progress.accept(frame.len(), 6).unwrap();
    drop(frame);
    outbox.finish_send(progress, 6).unwrap();
    assert_eq!(bytes.used(), 0);
}
