use std::task::{Context, Poll, Waker};

use super::super::port::channel;
use super::super::test_waker::{ProbeCounts, probing_waker};

#[test]
fn port_keeps_exactly_one_value_and_closes_both_sides() {
    let (sender, mut receiver) = channel();
    assert_eq!(sender.try_send(7), Ok(()));
    assert_eq!(sender.try_send(8), Err(8));

    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(7))));
    assert_eq!(receiver.poll_take(&mut context), Poll::Pending);

    drop(sender);
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(None)));
}

#[test]
fn sender_close_preserves_its_queued_value_for_one_receiver_drain() {
    let (sender, mut receiver) = channel();
    assert_eq!(sender.try_send(11), Ok(()));
    drop(sender);

    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(11))));
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(None)));
}

#[test]
fn receiver_drop_returns_a_pending_value_to_its_owner() {
    struct DropProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }

    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (sender, receiver) = channel();
    assert!(
        sender
            .try_send(DropProbe(std::sync::Arc::clone(&drops)))
            .is_ok()
    );
    drop(receiver);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Acquire), 1);
    assert!(
        sender
            .try_send(DropProbe(std::sync::Arc::clone(&drops)))
            .is_err()
    );
    assert_eq!(drops.load(std::sync::atomic::Ordering::Acquire), 2);
}

#[test]
fn ready_receive_rearms_notification_before_the_next_worker_result() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;

    struct Notifications(AtomicUsize);

    impl Wake for Notifications {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    let notifications = Arc::new(Notifications(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&notifications));
    let mut context = Context::from_waker(&waker);
    let (sender, mut receiver) = channel();
    assert_eq!(sender.try_send(1), Ok(()));
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(1))));

    // The aggregate can request the next event in this same turn and then
    // return Pending without polling this now-empty port a second time.
    assert_eq!(sender.try_send(2), Ok(()));
    assert_eq!(notifications.0.load(Ordering::Acquire), 1);
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(2))));
    assert_eq!(sender.try_send(3), Ok(()));
    assert_eq!(notifications.0.load(Ordering::Acquire), 2);
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(Some(3))));
    drop(sender);
    assert_eq!(notifications.0.load(Ordering::Acquire), 3);
    assert_eq!(receiver.poll_take(&mut context), Poll::Ready(Ok(None)));
}

#[test]
fn port_poll_paths_replace_and_wake_after_unlocking() {
    let (sender, mut receiver) = channel::<i32>();
    let lock = receiver.lock_probe_for_test();
    let first_counts = ProbeCounts::new();
    let first = probing_waker(std::sync::Arc::clone(&lock), first_counts.clone());
    assert_eq!(
        receiver.poll_closed(&mut Context::from_waker(first.waker())),
        Poll::Pending
    );
    assert_eq!(first.strong_count(), 2);
    drop(first);

    let second_counts = ProbeCounts::new();
    let second = probing_waker(lock, second_counts.clone());
    assert_eq!(
        receiver.poll_take(&mut Context::from_waker(second.waker())),
        Poll::Pending
    );
    assert_eq!(first_counts.values(), (1, 0, 0));
    assert_eq!(second.strong_count(), 2);
    drop(second);

    drop(sender);
    assert_eq!(second_counts.values(), (1, 1, 0));
    assert_eq!(
        receiver.poll_take(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(None))
    );
}

#[test]
fn ready_port_rearms_and_receiver_close_disposes_after_unlocking() {
    let (sender, mut receiver) = channel();
    sender.try_send(7).unwrap();
    let counts = ProbeCounts::new();
    let proposed = probing_waker(receiver.lock_probe_for_test(), counts.clone());
    assert_eq!(
        receiver.poll_take(&mut Context::from_waker(proposed.waker())),
        Poll::Ready(Ok(Some(7)))
    );
    assert_eq!(proposed.strong_count(), 2);
    drop(proposed);
    drop(receiver);
    assert_eq!(counts.values(), (1, 0, 0));
    assert_eq!(sender.try_send(8), Err(8));
}

#[test]
fn closed_and_poisoned_ports_drop_unused_wakers_after_unlocking() {
    let (sender, mut closed) = channel::<i32>();
    drop(sender);
    let closed_counts = ProbeCounts::new();
    let proposed = probing_waker(closed.lock_probe_for_test(), closed_counts.clone());
    assert_eq!(
        closed.poll_closed(&mut Context::from_waker(proposed.waker())),
        Poll::Ready(Ok(()))
    );
    drop(proposed);
    assert_eq!(closed_counts.values(), (1, 0, 0));

    let (_sender, mut poisoned) = channel::<i32>();
    poisoned.poison_for_test();
    let poison_counts = ProbeCounts::new();
    let proposed = probing_waker(poisoned.lock_probe_for_test(), poison_counts.clone());
    assert_eq!(
        poisoned.poll_take(&mut Context::from_waker(proposed.waker())),
        Poll::Ready(Err(super::super::SubscriptionWorkerRuntimeError::Invariant))
    );
    drop(proposed);
    assert_eq!(poison_counts.values(), (1, 0, 0));
}

#[test]
fn close_and_take_transfers_once_and_rejects_poisoned_values_outside_lock() {
    use super::super::SubscriptionWorkerRuntimeError;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let (sender, receiver) = channel();
    sender.try_send(7).unwrap();
    assert_eq!(receiver.close_and_take(), Ok(Some(7)));
    assert_eq!(receiver.close_and_take(), Ok(None));
    assert_eq!(sender.try_send(8), Err(8));

    struct Payload {
        locked: Arc<dyn Fn() -> bool + Send + Sync>,
        disposed: Arc<AtomicBool>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            assert!(!(self.locked)(), "payload destruction held the port mutex");
            self.disposed.store(true, Ordering::SeqCst);
        }
    }
    let (sender, receiver) = channel();
    let disposed = Arc::new(AtomicBool::new(false));
    assert!(
        sender
            .try_send(Payload {
                locked: receiver.lock_probe_for_test(),
                disposed: Arc::clone(&disposed),
            })
            .is_ok()
    );
    receiver.poison_for_test();
    assert!(matches!(
        receiver.close_and_take(),
        Err(SubscriptionWorkerRuntimeError::Invariant)
    ));
    assert!(disposed.load(Ordering::SeqCst));
}
