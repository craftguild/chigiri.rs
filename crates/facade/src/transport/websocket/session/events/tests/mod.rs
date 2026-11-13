//! Cutoff authority is finite and never recreates normal event emission.

mod cancellation;
mod fixture;

use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use crate::subscription::bridge_resources::BridgeOperationKind;
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::worker::websocket::WebSocketTerminalMessage;
use fixture::{enqueue, finish_cleanup, fixture, response, terminal};

#[test]
fn no_replacement_allows_only_pre_cutoff_events_to_drain_in_order() {
    let mut f = fixture();
    enqueue(&mut f.queue, "first", 0);
    enqueue(&mut f.queue, "second", 0);
    let mut terminal = terminal(f.active.take().unwrap(), WebSocketTerminalMessage::Complete);
    f.queue.retire(&terminal).unwrap();
    assert_eq!(
        f.queue.reserve_before_poll(0).err(),
        Some(SubscriptionOutboxSignal::Cancelled)
    );
    let mut context = Context::from_waker(Waker::noop());
    for (index, expected) in ["first", "second"].into_iter().enumerate() {
        assert!(f.queue.has_event());
        let frame = f.queue.begin_send(index as u64).unwrap().unwrap();
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains(expected)
        );
        assert!(!f.queue.is_drained());
        let clone = frame.clone();
        f.queue
            .record_progress(frame.as_bytes().len(), true, index as u64)
            .unwrap();
        drop(frame);
        assert!(
            f.queue
                .poll_finish_send(&mut context, index as u64)
                .is_pending()
        );
        drop(clone);
        assert_eq!(
            f.queue.poll_finish_send(&mut context, index as u64),
            Poll::Ready(Ok(true))
        );
        assert_eq!(f.queue.is_drained(), index == 1);
    }
    assert!(!f.queue.has_event());
    assert!(f.queue.begin_send(2).unwrap().is_none());
    finish_cleanup(&mut terminal);
}

#[test]
fn prior_reservation_cannot_be_used_to_encode_after_source_terminal_cutoff() {
    let mut f = fixture();
    enqueue(&mut f.queue, "admitted", 0);
    let reservation = f.queue.reserve_before_poll(0).unwrap();
    let terminal = terminal(f.active.take().unwrap(), WebSocketTerminalMessage::Complete);
    f.queue.retire(&terminal).unwrap();
    assert_eq!(
        f.queue
            .encode_and_enqueue(reservation, &response("late"), 0),
        Err(SubscriptionOutboxSignal::Cancelled)
    );
    let frame = f.queue.begin_send(0).unwrap().unwrap();
    assert!(
        !std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("late")
    );
}

#[test]
fn accepted_replacement_invalidates_old_unsent_data_but_not_new_emission() {
    let mut f = fixture();
    enqueue(&mut f.queue, "old", 0);
    let mut old = terminal(f.active.take().unwrap(), WebSocketTerminalMessage::Complete);
    f.queue.retire(&old).unwrap();
    let new = f
        .connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    // The root serializes this invalidation and final pre-start_send checks.
    f.queue.invalidate();
    assert!(new.emission_lease().can_emit());
    assert!(!f.queue.has_event());
    assert!(f.queue.begin_send(0).unwrap().is_none());
    assert!(f.queue.is_drained());
    assert_eq!(f.owner.delivery().byte_owner().used(), 0);
    finish_cleanup(&mut old);
    drop(old);
    assert_eq!(
        f.owner.delivery().snapshot().endpoint_active_subscriptions,
        1
    );
    assert!(new.emission_lease().can_emit());
}

#[test]
fn cancellation_or_foreign_terminal_cannot_grant_old_event_drain_authority() {
    let mut f = fixture();
    enqueue(&mut f.queue, "not-authorized", 0);
    let cancelled = terminal(
        f.active.take().unwrap(),
        WebSocketTerminalMessage::Cancelled,
    );
    assert_eq!(
        f.queue.retire(&cancelled),
        Err(SubscriptionOutboxSignal::Invariant)
    );
    assert!(!f.queue.has_event());
    let new = f
        .connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let foreign = terminal(new, WebSocketTerminalMessage::Complete);
    assert_eq!(
        f.queue.retire(&foreign),
        Err(SubscriptionOutboxSignal::Invariant)
    );
    f.queue.invalidate();
    assert!(f.queue.begin_send(0).unwrap().is_none());
}
