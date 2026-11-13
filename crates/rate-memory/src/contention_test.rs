//! Tests lock contention in the official provider without an async runtime.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use chigiri_rate::{
    OperationCost, QuotaKey, QuotaSubject, RateLimitRequest, RateLimiter, RatePolicy,
};

pub(super) fn assert_pending_drop_and_repoll<P, G>(provider: &P, policy: RatePolicy, guard: G)
where
    P: RateLimiter,
{
    let key = QuotaKey::new(b"official-contention".to_vec(), QuotaSubject::Missing, None).unwrap();
    let request = RateLimitRequest::new(&key, OperationCost::one(), policy);
    let dropped_count = Arc::new(WakeCount::default());
    let dropped_waker = Waker::from(Arc::clone(&dropped_count));
    let mut dropped_context = Context::from_waker(&dropped_waker);

    let mut dropped = provider.check_and_consume(request);
    assert!(matches!(
        dropped.as_mut().poll(&mut dropped_context),
        Poll::Pending
    ));
    assert!(matches!(
        dropped.as_mut().poll(&mut dropped_context),
        Poll::Pending
    ));
    assert_eq!(dropped_count.0.load(Ordering::SeqCst), 0);
    drop(dropped);

    let resumed_count = Arc::new(WakeCount::default());
    let resumed_waker = Waker::from(Arc::clone(&resumed_count));
    let mut resumed_context = Context::from_waker(&resumed_waker);
    let mut resumed = provider.check_and_consume(request);
    assert!(matches!(
        resumed.as_mut().poll(&mut resumed_context),
        Poll::Pending
    ));
    assert_eq!(resumed_count.0.load(Ordering::SeqCst), 0);
    drop(guard);
    assert_eq!(dropped_count.0.load(Ordering::SeqCst), 0);
    assert_eq!(resumed_count.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        resumed.as_mut().poll(&mut resumed_context),
        Poll::Ready(Ok(_))
    ));
}

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
