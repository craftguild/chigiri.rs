//! Queue admission, whole-entry eviction, and deferred metadata tests.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::SubscriptionOutboxSignal;
use super::fixture::{EVENT_LIMIT, assert_profile, counter, enqueue, outbox, response, settings};
use crate::subscription::delivery::EndpointByteOwner;

#[test]
fn real_eviction_survivor_gets_current_counter_in_every_profile() {
    for transport in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        let settings = settings(2, 100, 100);
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings,
            owner,
            SubscriptionOverflow::DropOldest,
            transport,
        );
        enqueue(&mut outbox, "evicted", 0).unwrap();
        enqueue(&mut outbox, "survivor", 1).unwrap();
        enqueue(&mut outbox, "newest", 2).unwrap();

        let (frame, mut progress) = outbox.begin_send(3).unwrap().unwrap();
        assert_profile(frame.as_bytes(), transport);
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains("survivor")
        );
        assert_eq!(counter(frame.as_bytes()), Some(1));
        assert!(progress.accept(frame.len(), 3).unwrap());
        drop(frame);
        outbox.finish_send(progress, 3).unwrap();

        let (frame, _) = {
            let (frame, progress) = outbox.begin_send(4).unwrap().unwrap();
            (frame, progress)
        };
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains("newest")
        );
        assert_eq!(counter(frame.as_bytes()), Some(1));
    }
}

#[test]
fn backpressure_uses_null_and_drop_oldest_starts_at_zero() {
    for (overflow, expected) in [
        (SubscriptionOverflow::Backpressure, None),
        (SubscriptionOverflow::DropOldest, Some(0)),
    ] {
        let settings = settings(2, 100, 100);
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings,
            owner,
            overflow,
            SubscriptionTransport::ServerSentEvents,
        );
        enqueue(&mut outbox, "kept", 0).unwrap();
        let (frame, _) = outbox.begin_send(1).unwrap().unwrap();
        assert_eq!(counter(frame.as_bytes()), expected);
    }
}

#[test]
fn backpressure_waits_before_poll_and_never_discards_a_yielded_result() {
    let settings = settings(1, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "first", 0).unwrap();
    assert_eq!(
        outbox.reserve_before_poll(1).err().unwrap(),
        SubscriptionOutboxSignal::Wait
    );
    let (frame, _) = outbox.begin_send(2).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("first")
    );
}

#[test]
fn a_foreign_reservation_fails_closed_and_releases_the_original_slot() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut first = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let mut second = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let foreign = first.reserve_before_poll(0).unwrap();
    assert_eq!(
        second
            .encode_and_enqueue(foreign, &response("foreign"), 0)
            .unwrap_err(),
        SubscriptionOutboxSignal::Invariant
    );
    assert!(first.reserve_before_poll(1).is_ok());
    assert_eq!(
        second.reserve_before_poll(1).err().unwrap(),
        SubscriptionOutboxSignal::Invariant
    );
}

#[test]
fn eof_or_pending_demand_reservation_does_not_evict_a_full_drop_oldest_queue() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "first", 0).unwrap();
    enqueue(&mut outbox, "second", 1).unwrap();
    let charged = owner.used();
    let reservation = outbox.reserve_before_poll(2).unwrap();
    assert_eq!(owner.used(), charged + EVENT_LIMIT);
    drop(reservation);
    assert_eq!(owner.used(), charged);
    let (frame, _) = outbox.begin_send(3).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_ref())
            .unwrap()
            .contains("first")
    );
    assert_eq!(counter(frame.as_ref()), Some(0));
}

#[test]
fn virtual_maximum_does_not_evict_before_a_small_actual_candidate() {
    let baseline = settings(8, 100, 100);
    let mut limits = *baseline.limits();
    limits.max_queued_bytes = 2 * EVENT_LIMIT;
    let settings = crate::subscription::settings::ValidatedSubscriptionSettings::new(
        limits,
        *baseline.protocol(),
    );
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, &"a".repeat(600), 0).unwrap();
    enqueue(&mut outbox, &"b".repeat(600), 1).unwrap();
    let before = owner.used();
    assert!(before + EVENT_LIMIT > limits.max_queued_bytes);
    let reservation = outbox.reserve_before_poll(2).unwrap();
    assert_eq!(owner.used(), before + EVENT_LIMIT);
    outbox
        .encode_and_enqueue(reservation, &response("small"), 2)
        .unwrap();
    let (frame, _) = outbox.begin_send(3).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_ref())
            .unwrap()
            .contains(&"a".repeat(600))
    );
    assert_eq!(counter(frame.as_ref()), Some(0));
}
