//! Registration drain Waker ownership at the process-reaper boundary.

use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use super::WorkerRegistration;
use super::generation::{GenerationSlot, WorkerGenerationSlot};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::test_waker::{ProbeCounts, probing_waker};

fn registration_with_generation() -> (Arc<WorkerRegistration>, WorkerGenerationSlot) {
    let registration = WorkerRegistration::new(1, Weak::new());
    let slot = GenerationSlot::new(Arc::downgrade(&registration), 1);
    registration
        .state
        .lock()
        .unwrap()
        .slots
        .insert(1, Arc::clone(&slot));
    (registration, WorkerGenerationSlot::new(slot))
}

#[test]
fn drain_waker_replacement_and_real_generation_wake_run_after_unlocking() {
    let (registration, generation) = registration_with_generation();
    let lock = registration.lock_probe_for_test();
    let first_counts = ProbeCounts::new();
    let first = probing_waker(Arc::clone(&lock), first_counts.clone());
    assert_eq!(
        registration.poll_drained(&mut Context::from_waker(first.waker())),
        Poll::Pending
    );
    assert_eq!(first.strong_count(), 2);
    drop(first);

    let second_counts = ProbeCounts::new();
    let second = probing_waker(lock, second_counts.clone());
    assert_eq!(
        registration.poll_drained(&mut Context::from_waker(second.waker())),
        Poll::Pending
    );
    assert_eq!(first_counts.values(), (1, 0, 0));
    assert_eq!(second.strong_count(), 2);
    drop(second);

    drop(generation);
    assert_eq!(second_counts.values(), (1, 1, 0));
    assert_eq!(
        registration.poll_drained(&mut Context::from_waker(std::task::Waker::noop())),
        Poll::Ready(Ok(()))
    );
}

#[test]
fn close_wakes_the_registered_drain_after_unlocking() {
    let (registration, generation) = registration_with_generation();
    let counts = ProbeCounts::new();
    let waker = probing_waker(registration.lock_probe_for_test(), counts.clone());
    assert_eq!(
        registration.poll_drained(&mut Context::from_waker(waker.waker())),
        Poll::Pending
    );
    drop(waker);
    registration.close();
    assert_eq!(counts.values(), (1, 1, 0));
    drop(generation);
}

#[test]
fn empty_and_poisoned_registrations_drop_unused_wakers_after_unlocking() {
    let empty = WorkerRegistration::new(1, Weak::new());
    let empty_counts = ProbeCounts::new();
    let waker = probing_waker(empty.lock_probe_for_test(), empty_counts.clone());
    assert_eq!(
        empty.poll_drained(&mut Context::from_waker(waker.waker())),
        Poll::Ready(Ok(()))
    );
    drop(waker);
    assert_eq!(empty_counts.values(), (1, 0, 0));

    let (poisoned, generation) = registration_with_generation();
    let state = Arc::clone(&poisoned);
    let _ = std::panic::catch_unwind(move || {
        let _guard = state.state.lock().unwrap();
        panic!("poison registration state for test");
    });
    let poison_counts = ProbeCounts::new();
    let waker = probing_waker(poisoned.lock_probe_for_test(), poison_counts.clone());
    assert_eq!(
        poisoned.poll_drained(&mut Context::from_waker(waker.waker())),
        Poll::Ready(Err(SubscriptionWorkerRuntimeError::Poisoned))
    );
    drop(waker);
    assert_eq!(poison_counts.values(), (1, 0, 0));
    drop(generation);
}
