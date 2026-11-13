//! Timer registration acknowledgement follows the exact external poll generation.

use super::*;
use crate::subscription::worker::test_waker::{ProbeCounts, probing_waker};

fn registered(
    state: &DeadlineState,
    role: DeadlineRole,
    generation: u64,
) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
    state.poll_registered(role, generation, &mut Context::from_waker(Waker::noop()))
}

#[test]
fn installation_alone_does_not_acknowledge_external_timer_registration() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    assert_eq!(registered(&state, DeadlineRole::Overall, 1), Poll::Pending);
    assert_eq!(poll(&state), Poll::Pending);
    assert_eq!(
        registered(&state, DeadlineRole::Overall, 1),
        Poll::Ready(Ok(()))
    );
}

#[test]
fn a_rearmed_phase_requires_its_own_external_observation() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let previous = state
        .arm(DeadlineRole::Phase, Box::pin(std::future::pending()))
        .unwrap();
    assert_eq!(poll(&state), Poll::Pending);
    assert_eq!(
        registered(&state, DeadlineRole::Phase, 1),
        Poll::Ready(Ok(()))
    );
    previous.clear().unwrap();
    let next = state
        .arm(DeadlineRole::Phase, Box::pin(std::future::pending()))
        .unwrap();
    assert!(matches!(
        registered(&state, DeadlineRole::Phase, 1),
        Poll::Ready(Err(_))
    ));
    assert_eq!(registered(&state, DeadlineRole::Phase, 2), Poll::Pending);
    assert_eq!(poll(&state), Poll::Pending);
    assert_eq!(
        registered(&state, DeadlineRole::Phase, 2),
        Poll::Ready(Ok(()))
    );
    next.clear().unwrap();
}

#[test]
fn an_inflight_first_poll_is_acknowledged_only_after_it_returns() {
    let (timer, gate) = blocked_timer(false);
    let state = DeadlineState::with_overall(timer).unwrap();
    let (expiry, registration, _) = during_poll(&state, gate, || {
        registered(&state, DeadlineRole::Overall, 1)
    });
    assert_eq!(registration, Poll::Pending);
    assert_eq!(expiry, Poll::Pending);
    assert_eq!(
        registered(&state, DeadlineRole::Overall, 1),
        Poll::Ready(Ok(()))
    );
}

#[test]
fn registration_waiters_are_woken_outside_the_deadline_lock() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let notifications = ProbeCounts::new();
    let waker = probing_waker(state.lock_probe_for_test(), notifications.clone());
    assert_eq!(
        state.poll_registered(
            DeadlineRole::Overall,
            1,
            &mut Context::from_waker(waker.waker())
        ),
        Poll::Pending
    );
    assert_eq!(poll(&state), Poll::Pending);
    drop(waker);
    assert_eq!(notifications.values(), (1, 1, 0));
    state.close();
    assert!(matches!(
        registered(&state, DeadlineRole::Overall, 1),
        Poll::Ready(Err(_))
    ));
}
