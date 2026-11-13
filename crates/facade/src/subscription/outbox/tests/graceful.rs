//! Shutdown discards backlog without pretending that it is DropOldest loss.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::fixture::{enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;

#[test]
fn graceful_discard_preserves_current_immutable_frame_charge_and_counter() {
    for transport in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        for overflow in [
            SubscriptionOverflow::Backpressure,
            SubscriptionOverflow::DropOldest,
        ] {
            let settings = settings(4, 100, 50);
            let owner = EndpointByteOwner::new(16 * 1024);
            let mut outbox = outbox(&settings, owner.clone(), overflow, transport);
            enqueue(&mut outbox, "current", 0).unwrap();
            let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
            let immutable = frame.as_ref().to_vec();
            let current_bytes = frame.as_ref().len();
            enqueue(&mut outbox, "waiting-a", 0).unwrap();
            enqueue(&mut outbox, "waiting-b", 0).unwrap();
            let counter = outbox.delivery_counter();
            assert!(owner.used() > current_bytes);
            assert!(outbox.discard_waiting());
            assert_eq!(owner.used(), current_bytes);
            assert_eq!(outbox.delivery_counter(), counter);
            assert!(outbox.take_drop_observations().next().is_none());
            assert_eq!(frame.as_ref(), immutable);
            assert!(!outbox.discard_waiting());
            progress.accept(current_bytes, 1).unwrap();
            drop(frame);
            outbox.finish_send(progress, 1).unwrap();
            assert!(outbox.is_empty());
            assert_eq!(owner.used(), 0);
        }
    }
}

#[test]
fn graceful_discard_reports_a_waiting_candidate_as_an_ordering_gap() {
    let settings = settings(1, 100, 50);
    let owner = EndpointByteOwner::new(16 * 1024);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "current", 0).unwrap();
    let (frame, _) = outbox.begin_send(0).unwrap().unwrap();
    enqueue(&mut outbox, "candidate", 0).unwrap();
    let current_bytes = frame.as_ref().len();
    assert!(owner.used() > current_bytes);
    assert!(outbox.discard_waiting());
    assert_eq!(owner.used(), current_bytes);
    assert_eq!(outbox.delivery_counter(), Some(0));
    outbox.cancel();
    assert_eq!(owner.used(), current_bytes);
    drop(frame);
    assert_eq!(owner.used(), 0);
}
