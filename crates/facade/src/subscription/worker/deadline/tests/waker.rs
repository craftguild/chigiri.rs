//! External Waker and timer disposal at the deadline-state lock boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::super::DeadlineRole;
use super::super::state::DeadlineState;
use crate::subscription::worker::test_waker::{ProbeCounts, probing_waker};

type LockCheck = Arc<dyn Fn() -> bool + Send + Sync + 'static>;
type DeferredLockCheck = Arc<Mutex<Option<LockCheck>>>;

#[test]
fn outer_and_proxy_wakers_are_replaced_after_unlocking() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let lock = state.lock_probe_for_test();
    let outer_first_counts = ProbeCounts::new();
    let outer_first = probing_waker(Arc::clone(&lock), outer_first_counts.clone());
    assert_eq!(
        state.poll_expired(&mut Context::from_waker(outer_first.waker())),
        Poll::Pending
    );
    assert_eq!(outer_first.strong_count(), 2);
    drop(outer_first);
    let outer_second = probing_waker(Arc::clone(&lock), ProbeCounts::new());
    assert_eq!(
        state.poll_expired(&mut Context::from_waker(outer_second.waker())),
        Poll::Pending
    );
    assert_eq!(outer_first_counts.values(), (1, 0, 0));

    let mut proxy = state.proxy(DeadlineRole::Overall).unwrap();
    let proxy_first_counts = ProbeCounts::new();
    let proxy_first = probing_waker(Arc::clone(&lock), proxy_first_counts.clone());
    assert_eq!(
        Pin::new(&mut proxy).poll(&mut Context::from_waker(proxy_first.waker())),
        Poll::Pending
    );
    drop(proxy_first);
    let proxy_second = probing_waker(lock, ProbeCounts::new());
    assert_eq!(
        Pin::new(&mut proxy).poll(&mut Context::from_waker(proxy_second.waker())),
        Poll::Pending
    );
    assert_eq!(proxy_first_counts.values(), (1, 0, 0));
}

struct TimerDropProbe {
    lock: DeferredLockCheck,
    drops: Arc<AtomicUsize>,
    under_lock: Arc<AtomicUsize>,
}

impl Future for TimerDropProbe {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}

impl Drop for TimerDropProbe {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        let probe = self.lock.lock().unwrap().clone().unwrap();
        if probe() {
            self.under_lock.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[test]
fn clear_drops_proxy_waker_and_armed_timer_after_unlocking() {
    let lock: DeferredLockCheck = Arc::new(Mutex::new(None));
    let drops = Arc::new(AtomicUsize::new(0));
    let under_lock = Arc::new(AtomicUsize::new(0));
    let state = DeadlineState::with_overall(Box::pin(TimerDropProbe {
        lock: Arc::clone(&lock),
        drops: Arc::clone(&drops),
        under_lock: Arc::clone(&under_lock),
    }))
    .unwrap();
    let state_lock = state.lock_probe_for_test();
    *lock.lock().unwrap() = Some(Arc::clone(&state_lock));

    let mut proxy = state.proxy(DeadlineRole::Overall).unwrap();
    let counts = ProbeCounts::new();
    let waker = probing_waker(state_lock, counts.clone());
    assert_eq!(
        Pin::new(&mut proxy).poll(&mut Context::from_waker(waker.waker())),
        Poll::Pending
    );
    drop(waker);
    proxy.clear().unwrap();
    assert_eq!(counts.values(), (1, 0, 0));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(under_lock.load(Ordering::SeqCst), 0);
}

#[test]
fn closed_stale_fired_and_poisoned_paths_drop_unused_wakers_after_unlocking() {
    let closed = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let closed_counts = ProbeCounts::new();
    let closed_waker = probing_waker(closed.lock_probe_for_test(), closed_counts.clone());
    closed.close();
    assert!(matches!(
        closed.poll_expired(&mut Context::from_waker(closed_waker.waker())),
        Poll::Ready(Err(_))
    ));
    drop(closed_waker);
    assert_eq!(closed_counts.values(), (1, 0, 0));

    let stale = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let stale_counts = ProbeCounts::new();
    let stale_waker = probing_waker(stale.lock_probe_for_test(), stale_counts.clone());
    let mut live = true;
    assert_eq!(
        stale.poll_proxy(
            DeadlineRole::Overall,
            99,
            &mut live,
            &mut Context::from_waker(stale_waker.waker()),
        ),
        Poll::Ready(())
    );
    assert!(!live);
    drop(stale_waker);
    assert_eq!(stale_counts.values(), (1, 0, 0));

    let fired = DeadlineState::with_overall(Box::pin(async {})).unwrap();
    let proxy = fired.proxy(DeadlineRole::Overall).unwrap();
    assert!(matches!(
        fired.poll_expired(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(_))
    ));
    let fired_counts = ProbeCounts::new();
    let fired_waker = probing_waker(fired.lock_probe_for_test(), fired_counts.clone());
    let mut live = true;
    assert_eq!(
        fired.poll_proxy(
            DeadlineRole::Overall,
            1,
            &mut live,
            &mut Context::from_waker(fired_waker.waker()),
        ),
        Poll::Ready(())
    );
    assert!(!live);
    drop(fired_waker);
    drop(proxy);
    assert_eq!(fired_counts.values(), (1, 0, 0));

    let poisoned = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    poisoned.poison_for_test();
    let poison_counts = ProbeCounts::new();
    let poison_waker = probing_waker(poisoned.lock_probe_for_test(), poison_counts.clone());
    assert!(matches!(
        poisoned.poll_expired(&mut Context::from_waker(poison_waker.waker())),
        Poll::Ready(Err(_))
    ));
    drop(poison_waker);
    assert_eq!(poison_counts.values(), (1, 0, 0));
}
