//! Bounded asynchronous mutex used by the in-memory providers.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use chigiri_rate::RateLimitProviderError;

use crate::{BucketLockBusy, BucketLockPoisoned};

const FREE: u8 = 0;
const REGISTERING: u8 = 1;
const WAITING: u8 = 2;
const NOTIFIED: u8 = 3;
const CLEARING: u8 = 4;

/// Waits up to a fixed limit; excess requests fail as provider failures.
const MAX_WAITERS: usize = 64;

pub(super) struct AsyncMutex<T> {
    inner: Mutex<T>,
    waiters: Box<[Waiter]>,
    wake_cursor: AtomicUsize,
}

impl<T> AsyncMutex<T> {
    pub(super) fn new(value: T) -> Self {
        let waiters = (0..MAX_WAITERS)
            .map(|_| Waiter::new())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            inner: Mutex::new(value),
            waiters,
            wake_cursor: AtomicUsize::new(0),
        }
    }

    pub(super) fn try_lock(&self) -> TryLockResult<AsyncMutexGuard<'_, T>> {
        match self.inner.try_lock() {
            Ok(guard) => Ok(AsyncMutexGuard::new(self, guard)),
            Err(TryLockError::Poisoned(error)) => Err(TryLockError::Poisoned(PoisonError::new(
                AsyncMutexGuard::new(self, error.into_inner()),
            ))),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        }
    }

    pub(super) fn lock(&self) -> LockFuture<'_, T> {
        LockFuture {
            mutex: self,
            waiter: None,
        }
    }

    fn reserve_waiter(&self) -> Option<usize> {
        let start = self.wake_cursor.load(Ordering::Relaxed) % self.waiters.len();
        (0..self.waiters.len())
            .map(|offset| (start + offset) % self.waiters.len())
            .find(|index| {
                self.waiters[*index]
                    .state
                    .compare_exchange(FREE, REGISTERING, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
    }

    fn arm_waiter(&self, index: usize, context: &Context<'_>) {
        let waiter = &self.waiters[index];
        let mut state = waiter.state.load(Ordering::Acquire);
        while state != REGISTERING {
            debug_assert!(matches!(state, WAITING | NOTIFIED));
            match waiter.state.compare_exchange(
                state,
                REGISTERING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => state = current,
            }
        }
        waiter.waker.register(context.waker());
        waiter.state.store(WAITING, Ordering::Release);
    }

    fn clear_waiter(&self, index: usize) -> u8 {
        let waiter = &self.waiters[index];
        let mut state = waiter.state.load(Ordering::Acquire);
        let previous = loop {
            debug_assert!(matches!(state, REGISTERING | WAITING | NOTIFIED));
            match waiter.state.compare_exchange(
                state,
                CLEARING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(previous) => break previous,
                Err(current) => state = current,
            }
        };
        drop(waiter.waker.take());
        waiter.state.store(FREE, Ordering::Release);
        previous
    }

    fn notify_one(&self) {
        let start = self.wake_cursor.fetch_add(1, Ordering::Relaxed) % self.waiters.len();
        for offset in 0..self.waiters.len() {
            let index = (start + offset) % self.waiters.len();
            let waiter = &self.waiters[index];
            if waiter
                .state
                .compare_exchange(WAITING, NOTIFIED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                waiter.waker.wake();
                break;
            }
        }
    }

    fn relay_cancelled_notification(&self) {
        match self.inner.try_lock() {
            Ok(guard) => drop(guard),
            Err(TryLockError::Poisoned(error)) => drop(error.into_inner()),
            Err(TryLockError::WouldBlock) => return,
        }
        self.notify_one();
    }
}

struct Waiter {
    state: AtomicU8,
    waker: AtomicWaker,
}

impl Waiter {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(FREE),
            waker: AtomicWaker::new(),
        }
    }
}

pub(super) struct AsyncMutexGuard<'a, T> {
    mutex: &'a AsyncMutex<T>,
    guard: Option<MutexGuard<'a, T>>,
}

impl<'a, T> AsyncMutexGuard<'a, T> {
    fn new(mutex: &'a AsyncMutex<T>, guard: MutexGuard<'a, T>) -> Self {
        Self {
            mutex,
            guard: Some(guard),
        }
    }
}

impl<T> Deref for AsyncMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("guard must remain present until drop")
    }
}

impl<T> DerefMut for AsyncMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_deref_mut()
            .expect("guard must remain present until drop")
    }
}

impl<T> Drop for AsyncMutexGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        self.mutex.notify_one();
    }
}

pub(super) struct LockFuture<'a, T> {
    mutex: &'a AsyncMutex<T>,
    waiter: Option<usize>,
}

impl<'a, T> Future for LockFuture<'a, T> {
    type Output = Result<AsyncMutexGuard<'a, T>, RateLimitProviderError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.mutex.try_lock() {
            Ok(guard) => {
                self.clear_waiter();
                return Poll::Ready(Ok(guard));
            }
            Err(TryLockError::Poisoned(error)) => {
                self.clear_waiter();
                drop(error);
                return Poll::Ready(Err(RateLimitProviderError::new(BucketLockPoisoned)));
            }
            Err(TryLockError::WouldBlock) => {}
        }

        let Some(index) = self.waiter.or_else(|| self.mutex.reserve_waiter()) else {
            return Poll::Ready(Err(RateLimitProviderError::new(BucketLockBusy)));
        };
        self.waiter = Some(index);
        self.mutex.arm_waiter(index, context);

        match self.mutex.try_lock() {
            Ok(guard) => {
                self.clear_waiter();
                Poll::Ready(Ok(guard))
            }
            Err(TryLockError::Poisoned(error)) => {
                self.clear_waiter();
                drop(error);
                Poll::Ready(Err(RateLimitProviderError::new(BucketLockPoisoned)))
            }
            Err(TryLockError::WouldBlock) => Poll::Pending,
        }
    }
}

impl<T> LockFuture<'_, T> {
    fn clear_waiter(&mut self) {
        if let Some(index) = self.waiter.take() {
            self.mutex.clear_waiter(index);
        }
    }
}

impl<T> Drop for LockFuture<'_, T> {
    fn drop(&mut self) {
        let Some(index) = self.waiter.take() else {
            return;
        };
        if self.mutex.clear_waiter(index) == NOTIFIED {
            self.mutex.relay_cancelled_notification();
        }
    }
}

#[cfg(test)]
mod tests;
