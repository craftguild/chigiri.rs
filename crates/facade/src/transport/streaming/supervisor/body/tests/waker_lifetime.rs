//! External Waker callbacks never run under the HTTP body mailbox mutex.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

mod probe;

use super::super::*;
use probe::{ProbeCounts, probing_waker};

fn port() -> Arc<BodyPort> {
    BodyPort::new(Arc::new(StreamClock::new()))
}

fn poison(port: &Arc<BodyPort>) {
    let port = Arc::clone(port);
    let panic = catch_unwind(AssertUnwindSafe(move || {
        let _guard = port.state.lock().unwrap();
        panic!("poison the HTTP body mailbox");
    }));
    assert!(panic.is_err());
}

fn take_driver(port: &BodyPort) -> Option<Waker> {
    let mut state = port
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.driver.take()
}

fn take_body(port: &BodyPort) -> Option<Waker> {
    let mut state = port
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.body.take()
}

#[test]
fn notifications_replace_and_drop_only_outside_the_mailbox_lock() {
    let port = port();
    let counts = ProbeCounts::new();
    port.state.lock().unwrap().driver =
        Some(probing_waker(Arc::clone(&port), counts.clone()).into_waker());
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    port.notifications(&mut context).unwrap();

    assert_eq!(replacement.strong_count(), 2);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(take_driver(&port));
    drop(replacement);
    assert_eq!(counts.values(), (2, 0, 0));
}

#[test]
fn poisoned_notifications_fail_without_leaking_the_replacement() {
    let (owner, active, physical, port) = super::fixture();
    port.offer(super::frame(&owner), physical.clone()).unwrap();
    let charged = owner.delivery().byte_owner().used();
    poison(&port);
    let counts = ProbeCounts::new();
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(matches!(
        port.notifications(&mut context),
        Err(HttpStreamingError::Invariant)
    ));

    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (0, 0, 0));
    {
        let state = port
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.body_closed);
        assert!(state.live);
        assert!(state.offered.is_some());
    }
    assert_eq!(owner.delivery().byte_owner().used(), charged);
    drop(replacement);
    assert_eq!(counts.values(), (1, 0, 0));
    port.finish(None);
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    drop((physical, active));
}

#[test]
fn closed_notifications_discard_both_wakers_after_unlocking() {
    let port = port();
    let counts = ProbeCounts::new();
    {
        let mut state = port.state.lock().unwrap();
        state.body_closed = true;
        state.driver = Some(probing_waker(Arc::clone(&port), counts.clone()).into_waker());
    }
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    let notifications = port.notifications(&mut context).unwrap();

    assert!(notifications.closed);
    assert!(take_driver(&port).is_none());
    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 0, 0));
}

#[test]
fn pending_body_poll_replaces_and_drops_only_outside_the_mailbox_lock() {
    let port = port();
    let counts = ProbeCounts::new();
    port.state.lock().unwrap().body =
        Some(probing_waker(Arc::clone(&port), counts.clone()).into_waker());
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(port.poll_body(&mut context).is_pending());

    assert_eq!(replacement.strong_count(), 2);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(take_body(&port));
    drop(replacement);
    assert_eq!(counts.values(), (2, 0, 0));
}

#[test]
fn ready_body_poll_discards_both_wakers_after_unlocking() {
    let port = port();
    let counts = ProbeCounts::new();
    {
        let mut state = port.state.lock().unwrap();
        state.error = Some(HttpStreamingError::Worker);
        state.body = Some(probing_waker(Arc::clone(&port), counts.clone()).into_waker());
    }
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(matches!(
        port.poll_body(&mut context),
        Poll::Ready(Some(Err(HttpStreamingError::Worker)))
    ));

    assert!(take_body(&port).is_none());
    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 0, 0));
}

#[test]
fn eof_body_poll_discards_both_wakers_after_unlocking() {
    let port = port();
    let counts = ProbeCounts::new();
    {
        let mut state = port.state.lock().unwrap();
        state.eof = true;
        state.body = Some(probing_waker(Arc::clone(&port), counts.clone()).into_waker());
    }
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(matches!(port.poll_body(&mut context), Poll::Ready(None)));

    assert!(take_body(&port).is_none());
    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 0, 0));
}

#[test]
fn poisoned_body_poll_fails_without_leaking_the_replacement() {
    let (owner, active, physical, port) = super::fixture();
    port.offer(super::frame(&owner), physical.clone()).unwrap();
    let charged = owner.delivery().byte_owner().used();
    poison(&port);
    let counts = ProbeCounts::new();
    let replacement = probing_waker(Arc::clone(&port), counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(matches!(
        port.poll_body(&mut context),
        Poll::Ready(Some(Err(HttpStreamingError::Invariant)))
    ));

    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (0, 0, 0));
    {
        let state = port
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.body_closed);
        assert!(state.live);
        assert!(state.offered.is_some());
    }
    assert_eq!(owner.delivery().byte_owner().used(), charged);
    drop(replacement);
    assert_eq!(counts.values(), (1, 0, 0));
    port.finish(None);
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    drop((physical, active));
}

#[test]
fn finish_and_close_wake_only_after_unlocking() {
    let body_port = port();
    let body_counts = ProbeCounts::new();
    let body_waker = probing_waker(Arc::clone(&body_port), body_counts.clone());
    let mut body_context = Context::from_waker(body_waker.waker());
    assert!(body_port.poll_body(&mut body_context).is_pending());
    body_port.finish(None);
    assert_eq!(body_counts.values(), (0, 1, 0));
    drop(body_waker);

    let driver_port = port();
    let driver_counts = ProbeCounts::new();
    let driver_waker = probing_waker(Arc::clone(&driver_port), driver_counts.clone());
    let mut driver_context = Context::from_waker(driver_waker.waker());
    driver_port.notifications(&mut driver_context).unwrap();
    driver_port.close_body();
    assert_eq!(driver_counts.values(), (0, 1, 0));
    drop(driver_waker);
}
