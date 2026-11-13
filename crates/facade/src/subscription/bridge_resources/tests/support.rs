use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::super::DetachedCleanup;

#[derive(Clone, Default)]
pub(super) struct ManualDeadline {
    ready: Arc<AtomicBool>,
    requested: Arc<Mutex<Vec<Duration>>>,
}

impl ManualDeadline {
    pub(super) fn expire(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub(super) fn requested(&self) -> Vec<Duration> {
        self.requested.lock().unwrap().clone()
    }
}

impl DeadlineFactory for ManualDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.requested.lock().unwrap().push(duration);
        let ready = Arc::clone(&self.ready);
        Box::pin(std::future::poll_fn(move |_| {
            if ready.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

#[derive(Clone, Default)]
pub(super) struct CleanupProbe {
    ready: Arc<AtomicBool>,
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl CleanupProbe {
    pub(super) fn ready() -> Self {
        let probe = Self::default();
        probe.release();
        probe
    }

    pub(super) fn release(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub(super) fn polls(&self) -> usize {
        self.polls.load(Ordering::Acquire)
    }

    pub(super) fn drops(&self) -> usize {
        self.drops.load(Ordering::Acquire)
    }

    pub(super) fn cleanup(&self) -> DetachedCleanup {
        self.cleanup_with(())
    }

    pub(super) fn cleanup_with<T>(&self, owner: T) -> DetachedCleanup
    where
        T: Send + 'static,
    {
        DetachedCleanup::new(ControlledCleanup {
            probe: self.clone(),
            _owner: owner,
        })
    }
}

struct ControlledCleanup<T> {
    probe: CleanupProbe,
    _owner: T,
}

impl<T> Future for ControlledCleanup<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.probe.polls.fetch_add(1, Ordering::AcqRel);
        if self.probe.ready.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl<T> Drop for ControlledCleanup<T> {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::AcqRel);
    }
}

pub(super) fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

pub(super) fn poll_cleanup_driver(
    driver: &super::super::BridgeCleanupDriver,
) -> Poll<Result<bool, super::super::BridgeCleanupError>> {
    driver.poll_one(&mut Context::from_waker(Waker::noop()))
}
