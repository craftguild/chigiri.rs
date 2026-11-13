//! Exact terminal-counter freeze with bounded prior-frame delivery.

use chigiri_http::{__encode_subscription_event, SubscriptionTransport};
use chigiri_runtime::SubscriptionOverflow;

use super::fixture::{EVENT_LIMIT, counter, enqueue, outbox, response, settings};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::settings::ValidatedSubscriptionSettings;

fn transports() -> [SubscriptionTransport; 3] {
    [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ]
}

#[test]
fn freeze_is_idempotent_and_prior_frames_keep_the_exact_counter() {
    for transport in transports() {
        let mut outbox = outbox(
            &settings(1, 100, 100),
            EndpointByteOwner::new(16 * EVENT_LIMIT),
            SubscriptionOverflow::DropOldest,
            transport,
        );
        enqueue(&mut outbox, "first", 0).unwrap();
        enqueue(&mut outbox, "survivor", 1).unwrap();
        assert_eq!(outbox.take_drop_observations().collect::<Vec<_>>(), [1]);

        assert_eq!(outbox.freeze_terminal_counter(), 1);
        assert_eq!(outbox.freeze_terminal_counter(), 1);
        assert_eq!(outbox.delivery_counter(), Some(1));
        assert!(outbox.take_drop_observations().next().is_none());
        assert_eq!(
            outbox.reserve_before_poll(2).err(),
            Some(SubscriptionOutboxSignal::Cancelled)
        );

        let (frame, mut progress) = outbox.begin_send(2).unwrap().unwrap();
        assert_eq!(counter(frame.as_bytes()), Some(1));
        assert!(progress.accept(frame.len(), 2).unwrap());
        drop(frame);
        outbox.finish_send(progress, 2).unwrap();
        assert!(outbox.begin_send(3).unwrap().is_none());
        assert_eq!(outbox.freeze_terminal_counter(), 1);
    }
}

#[test]
fn frozen_queue_age_terminates_without_evicting_or_incrementing() {
    let mut outbox = outbox(
        &settings(2, 10, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "queued", 0).unwrap();
    assert_eq!(outbox.freeze_terminal_counter(), 0);
    assert_eq!(outbox.next_deadline().unwrap(), Some(10));
    assert_eq!(
        outbox.on_timer(10),
        Err(SubscriptionOutboxSignal::QueueAgeExpired)
    );
    assert_eq!(outbox.freeze_terminal_counter(), 0);
    assert!(outbox.take_drop_observations().next().is_none());
}

#[test]
fn frozen_partial_send_keeps_the_original_stall_deadline() {
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings(2, 100, 10),
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::Multipart,
    );
    enqueue(&mut outbox, "partial", 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(!progress.accept(1, 0).unwrap());
    assert_eq!(outbox.freeze_terminal_counter(), 0);
    assert_eq!(outbox.next_deadline().unwrap(), Some(10));
    assert_eq!(
        outbox.on_timer(10),
        Err(SubscriptionOutboxSignal::SendStalled)
    );
    assert!(outbox.take_drop_observations().next().is_none());
    assert_eq!(owner.used(), frame.len());
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn freeze_drops_only_a_waiting_candidate_and_preserves_physical_send_charge() {
    let baseline = settings(2, 100, 10);
    let mut limits = *baseline.limits();
    limits.max_queued_bytes = EVENT_LIMIT;
    let settings = ValidatedSubscriptionSettings::new(limits, *baseline.protocol());
    let owner = EndpointByteOwner::new(4 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, &"a".repeat(600), 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(progress.accept(frame.len(), 1).unwrap());
    let reservation = outbox.reserve_before_poll(2).unwrap();
    outbox
        .encode_and_enqueue(reservation, &response(&"b".repeat(600)), 2)
        .unwrap();
    let encoded = __encode_subscription_event(
        SubscriptionTransport::ServerSentEvents,
        None,
        &response(&"b".repeat(600)),
        EVENT_LIMIT,
    )
    .unwrap();
    assert_eq!(owner.used(), frame.len() + encoded.len());

    assert_eq!(outbox.freeze_terminal_counter(), 0);
    assert_eq!(owner.used(), frame.len());
    drop(frame);
    outbox.finish_send(progress, 3).unwrap();
    assert_eq!(owner.used(), 0);
    assert!(outbox.take_drop_observations().next().is_none());
}

#[test]
fn prefreeze_source_reservation_can_finish_http_without_new_demand() {
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
        let reservation = outbox.reserve_before_poll(0).unwrap();
        assert_eq!(outbox.freeze_terminal_counter(), 0);
        let terminal = outbox.finish_http_source(reservation).unwrap();
        assert_eq!(outbox.freeze_terminal_counter(), 0);
        drop(terminal);
    }
}
