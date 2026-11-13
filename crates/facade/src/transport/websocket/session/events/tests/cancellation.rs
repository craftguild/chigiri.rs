//! An already-started frame survives invalidation without a new progress clock.

use std::task::{Context, Poll, Waker};

use super::fixture::{enqueue, finish_cleanup, fixture, terminal};
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::worker::websocket::WebSocketTerminalMessage;

#[test]
fn invalidation_drops_unsent_events_but_finishes_original_partial_frame() {
    let mut f = fixture();
    enqueue(&mut f.queue, "started", 0);
    enqueue(&mut f.queue, "never-start", 0);
    let frame = f.queue.begin_send(0).unwrap().unwrap();
    let length = frame.as_bytes().len();
    f.queue.record_progress(1, false, 1).unwrap();
    let mut terminal = terminal(f.active.take().unwrap(), WebSocketTerminalMessage::Complete);
    f.queue.retire(&terminal).unwrap();
    f.queue.invalidate();
    finish_cleanup(&mut terminal);
    drop(terminal);
    assert_eq!(f.owner.delivery().byte_owner().used(), length);
    assert_eq!(
        f.owner.delivery().snapshot().endpoint_active_subscriptions,
        1
    );
    assert_eq!(f.queue.next_deadline(), Ok(Some(11)));
    assert_eq!(f.queue.on_timer(10), Ok(()));
    f.queue.record_progress(length - 1, true, 10).unwrap();
    assert_eq!(f.queue.next_deadline(), Ok(None));
    let mut context = Context::from_waker(Waker::noop());
    assert!(f.queue.poll_finish_send(&mut context, 10).is_pending());
    drop(frame);
    assert_eq!(
        f.queue.poll_finish_send(&mut context, 10),
        Poll::Ready(Ok(true))
    );
    assert!(f.queue.is_drained());
    assert!(f.queue.begin_send(10).unwrap().is_none());
    assert_eq!(f.owner.delivery().byte_owner().used(), 0);
    assert_eq!(
        f.owner.delivery().snapshot().endpoint_active_subscriptions,
        0
    );
}

#[test]
fn retired_original_timeout_wins_simultaneously_ready_final_progress() {
    let mut f = fixture();
    enqueue(&mut f.queue, "started", 0);
    let frame = f.queue.begin_send(0).unwrap().unwrap();
    f.queue.record_progress(1, false, 1).unwrap();
    let terminal = terminal(f.active.take().unwrap(), WebSocketTerminalMessage::Complete);
    f.queue.retire(&terminal).unwrap();
    f.queue.invalidate();
    assert_eq!(f.queue.next_deadline(), Ok(Some(11)));
    assert_eq!(
        f.queue
            .record_progress(frame.as_bytes().len() - 1, true, 11),
        Err(SubscriptionOutboxSignal::SendStalled)
    );
    assert_eq!(
        f.owner.delivery().byte_owner().used(),
        frame.as_bytes().len()
    );
}

#[test]
fn flush_before_full_payload_is_an_invariant_failure_not_completion() {
    let mut f = fixture();
    enqueue(&mut f.queue, "started", 0);
    let _frame = f.queue.begin_send(0).unwrap().unwrap();
    assert_eq!(
        f.queue.record_progress(1, true, 0),
        Err(SubscriptionOutboxSignal::Invariant)
    );
    assert!(!f.queue.is_drained());
}
