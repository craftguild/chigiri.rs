//! Exact once-only observations at the common successful-eviction point.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::{SubscriptionOutbox, SubscriptionOutboxSignal};
use super::fixture::{EVENT_LIMIT, assert_profile, counter, enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;

const MAX_EXACT_JSON_INTEGER: u64 = (1_u64 << 53) - 1;

fn transports() -> [SubscriptionTransport; 3] {
    [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ]
}

fn totals(outbox: &mut SubscriptionOutbox) -> Vec<u64> {
    outbox.take_drop_observations().collect()
}

#[test]
fn successful_evictions_form_one_move_only_range_in_every_transport() {
    for transport in transports() {
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings(1, 100, 100),
            owner.clone(),
            SubscriptionOverflow::DropOldest,
            transport,
        );
        enqueue(&mut outbox, "first", 0).unwrap();
        enqueue(&mut outbox, "second", 1).unwrap();
        enqueue(&mut outbox, "third", 2).unwrap();

        assert_eq!(totals(&mut outbox), [1, 2]);
        assert!(totals(&mut outbox).is_empty());
        enqueue(&mut outbox, "fourth", 3).unwrap();
        let charged = owner.used();
        assert_eq!(totals(&mut outbox), [3]);
        assert!(totals(&mut outbox).is_empty());
        assert_eq!(owner.used(), charged);

        let (frame, _) = outbox.begin_send(4).unwrap().unwrap();
        assert_profile(frame.as_bytes(), transport);
        assert_eq!(counter(frame.as_bytes()), Some(3));
        assert_eq!(owner.used(), frame.len());
        assert!(totals(&mut outbox).is_empty());
    }
}

#[test]
fn cancellation_preserves_pending_totals_and_the_frozen_counter() {
    for transport in transports() {
        let mut outbox = outbox(
            &settings(1, 100, 100),
            EndpointByteOwner::new(16 * EVENT_LIMIT),
            SubscriptionOverflow::DropOldest,
            transport,
        );
        enqueue(&mut outbox, "first", 0).unwrap();
        enqueue(&mut outbox, "second", 1).unwrap();
        enqueue(&mut outbox, "third", 2).unwrap();

        outbox.cancel();
        assert_eq!(outbox.delivery_counter(), Some(2));
        assert_eq!(totals(&mut outbox), [1, 2]);
        assert!(totals(&mut outbox).is_empty());
        assert_eq!(
            outbox.reserve_before_poll(3).err().unwrap(),
            SubscriptionOutboxSignal::Cancelled
        );
        assert!(totals(&mut outbox).is_empty());
    }
}

#[test]
fn failure_after_one_success_does_not_infer_totals_from_the_final_counter() {
    let mut outbox = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "first", 0).unwrap();
    enqueue(&mut outbox, "second", 1).unwrap();
    outbox
        .controller
        .as_mut()
        .unwrap()
        .set_dropped_for_test(MAX_EXACT_JSON_INTEGER);

    assert_eq!(
        enqueue(&mut outbox, "counter-exhausted", 2).unwrap_err(),
        SubscriptionOutboxSignal::CounterExhausted
    );
    assert_eq!(outbox.delivery_counter(), Some(MAX_EXACT_JSON_INTEGER));
    assert_eq!(totals(&mut outbox), [1]);
    assert!(totals(&mut outbox).is_empty());
    assert_eq!(
        outbox.reserve_before_poll(3).err().unwrap(),
        SubscriptionOutboxSignal::CounterExhausted
    );
    assert!(totals(&mut outbox).is_empty());
}

#[test]
fn backpressure_and_partial_inflight_ownership_add_no_synthetic_drop() {
    let mut backpressure = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut backpressure, "first", 0).unwrap();
    assert_eq!(
        backpressure.reserve_before_poll(1).err().unwrap(),
        SubscriptionOutboxSignal::Wait
    );
    assert!(totals(&mut backpressure).is_empty());
    assert_eq!(backpressure.delivery_counter(), None);

    let mut drop_oldest = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::WebSocket,
    );
    enqueue(&mut drop_oldest, "in-flight", 0).unwrap();
    let (frame, mut progress) = drop_oldest.begin_send(1).unwrap().unwrap();
    assert!(!progress.accept(1, 1).unwrap());
    enqueue(&mut drop_oldest, "whole-evicted", 2).unwrap();
    enqueue(&mut drop_oldest, "newest", 3).unwrap();

    assert_eq!(totals(&mut drop_oldest), [1]);
    assert_eq!(counter(frame.as_bytes()), Some(0));
    assert_eq!(drop_oldest.delivery_counter(), Some(1));
    drop(frame);
}

#[test]
fn http_controller_take_retains_the_pending_observation() {
    for transport in [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        let mut outbox = outbox(
            &settings(1, 100, 100),
            EndpointByteOwner::new(16 * EVENT_LIMIT),
            SubscriptionOverflow::DropOldest,
            transport,
        );
        enqueue(&mut outbox, "first", 0).unwrap();
        enqueue(&mut outbox, "survivor", 1).unwrap();
        let (frame, mut progress) = outbox.begin_send(2).unwrap().unwrap();
        assert!(progress.accept(frame.len(), 2).unwrap());
        drop(frame);
        outbox.finish_send(progress, 2).unwrap();
        let reservation = outbox.reserve_before_poll(3).unwrap();

        let terminal = outbox.finish_http_source(reservation).unwrap();
        assert_eq!(outbox.delivery_counter(), Some(1));
        assert_eq!(totals(&mut outbox), [1]);
        assert!(totals(&mut outbox).is_empty());
        drop(terminal);
    }
}
