//! Locked slot transitions with timer polling performed outside the lock.

mod clear;
mod polling;
mod registered;

use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use chigiri_http::BoxFuture;

use super::proxy::MonitoredDeadline;
use super::{DeadlineRole, FiredDeadline, HttpPhaseDeadline};
use crate::subscription::worker::error::SubscriptionWorkerRuntimeError;

pub(super) struct DeadlineState {
    inner: Mutex<DeadlineInner>,
}

struct DeadlineInner {
    slots: [DeadlineSlot; 4],
    outer_waker: Option<Waker>,
    closed: bool,
    corrupted: bool,
}

struct DeadlineSlot {
    generation: u64,
    observed: bool,
    http_phase: Option<HttpPhaseDeadline>,
    timer: TimerState,
    proxy_waker: Option<Waker>,
}

enum TimerState {
    Empty,
    Armed(BoxFuture<'static, ()>),
    Polling { clear: bool },
    Fired,
}

impl DeadlineState {
    pub(super) fn with_overall(
        overall: BoxFuture<'static, ()>,
    ) -> Result<Arc<Self>, SubscriptionWorkerRuntimeError> {
        let state = Self::empty();
        state.install(DeadlineRole::Overall, overall, None)?;
        Ok(state)
    }

    pub(super) fn with_setup(
        overall: BoxFuture<'static, ()>,
        setup: BoxFuture<'static, ()>,
    ) -> Result<Arc<Self>, SubscriptionWorkerRuntimeError> {
        let state = Self::empty();
        state.install(DeadlineRole::Overall, overall, None)?;
        state.install(DeadlineRole::Setup, setup, None)?;
        Ok(state)
    }

    pub(super) fn with_setup_only(
        setup: BoxFuture<'static, ()>,
    ) -> Result<Arc<Self>, SubscriptionWorkerRuntimeError> {
        let state = Self::empty();
        state.install(DeadlineRole::Setup, setup, None)?;
        Ok(state)
    }

    fn empty() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(DeadlineInner {
                slots: std::array::from_fn(|_| DeadlineSlot {
                    generation: 0,
                    observed: false,
                    http_phase: None,
                    timer: TimerState::Empty,
                    proxy_waker: None,
                }),
                outer_waker: None,
                closed: false,
                corrupted: false,
            }),
        })
    }

    pub(super) fn arm(
        self: &Arc<Self>,
        role: DeadlineRole,
        timer: BoxFuture<'static, ()>,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        self.install(role, timer, None)?;
        self.proxy(role)
    }

    pub(super) fn arm_http_phase(
        self: &Arc<Self>,
        timer: BoxFuture<'static, ()>,
        phase: HttpPhaseDeadline,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        self.install(DeadlineRole::Phase, timer, Some(phase))?;
        self.proxy(DeadlineRole::Phase)
    }

    pub(super) fn proxy(
        self: &Arc<Self>,
        role: DeadlineRole,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        let inner = self.lock()?;
        let slot = &inner.slots[role.index()];
        if inner.closed
            || !matches!(
                slot.timer,
                TimerState::Armed(_) | TimerState::Polling { clear: false } | TimerState::Fired
            )
        {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        Ok(MonitoredDeadline::new(
            Arc::clone(self),
            role,
            slot.generation,
        ))
    }

    fn install(
        &self,
        role: DeadlineRole,
        timer: BoxFuture<'static, ()>,
        http_phase: Option<HttpPhaseDeadline>,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        let mut inner = self.lock()?;
        if inner.closed {
            return Err(SubscriptionWorkerRuntimeError::Shutdown);
        }
        let slot = &mut inner.slots[role.index()];
        if !matches!(
            slot.timer,
            TimerState::Empty | TimerState::Polling { clear: true }
        ) {
            inner.corrupted = true;
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        slot.generation = slot
            .generation
            .checked_add(1)
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
        slot.observed = false;
        slot.http_phase = http_phase;
        slot.timer = TimerState::Armed(timer);
        let outer = inner.outer_waker.take();
        drop(inner);
        if let Some(waker) = outer {
            waker.wake();
        }
        Ok(())
    }

    pub(super) fn poll_expired(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Result<FiredDeadline, SubscriptionWorkerRuntimeError>> {
        let mut replacement = Some(context.waker().clone());
        let mut inner = match self.lock() {
            Ok(inner) => inner,
            Err(error) => {
                drop(replacement);
                return Poll::Ready(Err(error));
            }
        };
        if inner.closed || inner.corrupted {
            drop(inner);
            drop(replacement);
            return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
        }
        // Installations during an out-of-lock poll must schedule another turn,
        // including replacement of the role that this turn already visited.
        let previous = inner
            .outer_waker
            .replace(replacement.take().expect("proposed outer deadline waker"));
        drop(inner);
        drop(previous);
        for role in DeadlineRole::all() {
            match self.poll_role(role, context) {
                Poll::Ready(result) => return Poll::Ready(result),
                Poll::Pending => {}
            }
        }
        let inner = match self.lock() {
            Ok(inner) => inner,
            Err(error) => return Poll::Ready(Err(error)),
        };
        if inner.closed || inner.corrupted {
            return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
        }
        Poll::Pending
    }

    pub(super) fn poll_proxy(
        &self,
        role: DeadlineRole,
        generation: u64,
        live: &mut bool,
        context: &mut Context<'_>,
    ) -> Poll<()> {
        let mut replacement = Some(context.waker().clone());
        let mut inner = self.lock_recover();
        let closed = inner.closed;
        let slot = &mut inner.slots[role.index()];
        let (result, previous) =
            if closed || slot.generation != generation || matches!(slot.timer, TimerState::Fired) {
                *live = false;
                (Poll::Ready(()), None)
            } else {
                let previous = slot
                    .proxy_waker
                    .replace(replacement.take().expect("proposed deadline proxy waker"));
                (Poll::Pending, previous)
            };
        drop(inner);
        drop(previous);
        drop(replacement);
        result
    }

    pub(super) fn close(&self) {
        let mut inner = self.lock_recover();
        inner.closed = true;
        let outer = inner.outer_waker.take();
        let proxies: [Option<Waker>; 4] =
            std::array::from_fn(|index| inner.slots[index].proxy_waker.take());
        drop(inner);
        if let Some(waker) = outer {
            waker.wake();
        }
        for waker in proxies.into_iter().flatten() {
            waker.wake();
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, DeadlineInner>, SubscriptionWorkerRuntimeError> {
        self.inner
            .lock()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)
    }

    fn lock_recover(&self) -> MutexGuard<'_, DeadlineInner> {
        match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[cfg(test)]
    pub(super) fn lock_probe_for_test(
        self: &Arc<Self>,
    ) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        let state = Arc::downgrade(self);
        Arc::new(move || {
            state.upgrade().is_some_and(|state| {
                matches!(
                    state.inner.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                )
            })
        })
    }

    #[cfg(test)]
    pub(super) fn poison_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.inner.lock().unwrap();
            panic!("poison deadline state for test");
        }));
    }
}
