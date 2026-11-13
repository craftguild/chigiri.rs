use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use super::{AsyncMutex, MAX_WAITERS};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn waker(counter: &Arc<WakeCount>) -> Waker {
    Waker::from(Arc::clone(counter))
}

#[test]
fn waiter_count_and_single_lock_release_keep_wakeups_bounded() {
    let mutex = AsyncMutex::new(());
    let guard = mutex.try_lock().unwrap();
    let counters = (0..=MAX_WAITERS)
        .map(|_| Arc::new(WakeCount::default()))
        .collect::<Vec<_>>();
    let mut futures = (0..=MAX_WAITERS)
        .map(|_| Box::pin(mutex.lock()))
        .collect::<Vec<_>>();

    for (future, counter) in futures.iter_mut().zip(&counters).take(MAX_WAITERS) {
        let waker = waker(counter);
        let mut context = Context::from_waker(&waker);
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    }
    let overflow_waker = waker(&counters[MAX_WAITERS]);
    let mut overflow_context = Context::from_waker(&overflow_waker);
    assert!(matches!(
        futures[MAX_WAITERS].as_mut().poll(&mut overflow_context),
        Poll::Ready(Err(_))
    ));

    drop(guard);
    let wake_total = counters
        .iter()
        .map(|counter| counter.0.load(Ordering::SeqCst))
        .sum::<usize>();
    assert_eq!(wake_total, 1);
}

#[test]
fn dropping_a_notified_future_transfers_lock_release_to_the_next_waiter() {
    let mutex = AsyncMutex::new(());
    let guard = mutex.try_lock().unwrap();
    let first_count = Arc::new(WakeCount::default());
    let second_count = Arc::new(WakeCount::default());
    let first_waker = waker(&first_count);
    let second_waker = waker(&second_count);
    let mut first_context = Context::from_waker(&first_waker);
    let mut second_context = Context::from_waker(&second_waker);
    let mut first = Box::pin(mutex.lock());
    let mut second = Box::pin(mutex.lock());
    assert!(matches!(
        first.as_mut().poll(&mut first_context),
        Poll::Pending
    ));
    assert!(matches!(
        second.as_mut().poll(&mut second_context),
        Poll::Pending
    ));

    drop(guard);
    assert_eq!(first_count.0.load(Ordering::SeqCst), 1);
    assert_eq!(second_count.0.load(Ordering::SeqCst), 0);
    drop(first);
    assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        second.as_mut().poll(&mut second_context),
        Poll::Ready(Ok(_))
    ));
}

#[test]
fn dropping_a_pending_future_releases_its_waker_and_removes_its_slot() {
    let mutex = AsyncMutex::new(());
    let guard = mutex.try_lock().unwrap();
    let counter = Arc::new(WakeCount::default());
    let weak = Arc::downgrade(&counter);
    {
        let waker = waker(&counter);
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(mutex.lock());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        drop(future);
    }
    drop(counter);
    assert!(weak.upgrade().is_none());

    let replacement_count = Arc::new(WakeCount::default());
    let replacement_waker = waker(&replacement_count);
    let mut replacement_context = Context::from_waker(&replacement_waker);
    let mut replacement = Box::pin(mutex.lock());
    assert!(matches!(
        replacement.as_mut().poll(&mut replacement_context),
        Poll::Pending
    ));
    drop(guard);
    assert_eq!(replacement_count.0.load(Ordering::SeqCst), 1);
}

#[test]
fn poisoning_wakes_all_waiters_with_a_provider_failure() {
    let mutex = AsyncMutex::new(());
    let first_count = Arc::new(WakeCount::default());
    let second_count = Arc::new(WakeCount::default());
    let first_waker = waker(&first_count);
    let second_waker = waker(&second_count);
    let mut first_context = Context::from_waker(&first_waker);
    let mut second_context = Context::from_waker(&second_waker);
    let mut first = Box::pin(mutex.lock());
    let mut second = Box::pin(mutex.lock());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex.try_lock().unwrap();
        assert!(matches!(
            first.as_mut().poll(&mut first_context),
            Poll::Pending
        ));
        assert!(matches!(
            second.as_mut().poll(&mut second_context),
            Poll::Pending
        ));
        panic!("poison the lock for this test");
    }));
    assert!(result.is_err());
    assert_eq!(first_count.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        first.as_mut().poll(&mut first_context),
        Poll::Ready(Err(_))
    ));
    assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        second.as_mut().poll(&mut second_context),
        Poll::Ready(Err(_))
    ));
}
