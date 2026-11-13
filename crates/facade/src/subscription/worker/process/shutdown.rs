//! Recoverable exclusive ownership of the final process drain.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::{ProcessReaper, ProcessStatus, lock_process};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;

#[cfg(test)]
use super::{ProcessShared, ProcessState};

/// Exclusive completion owner for the process subscription reaper.
///
/// Only one value may exist at a time. Dropping it before completion releases
/// the ownership lease without restarting the drain, so a later explicit call
/// can resume and join the same OS thread. After the join, both registration and
/// subsequent shutdown attempts fail with `SubscriptionWorkerRuntimeError::Shutdown`.
#[must_use = "process shutdown must be awaited before destroying Tokio runtimes"]
#[doc(hidden)]
pub struct SubscriptionProcessShutdown {
    process: Arc<ProcessReaper>,
    owns_shutdown: bool,
}

impl SubscriptionProcessShutdown {
    pub(super) const fn new(process: Arc<ProcessReaper>) -> Self {
        Self {
            process,
            owns_shutdown: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn lock_probe_for_test(&self) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        let shared = Arc::downgrade(&self.process.shared);
        Arc::new(move || {
            shared.upgrade().is_some_and(|shared| {
                matches!(
                    shared.state.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                )
            })
        })
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test(poison_lock: bool) -> Self {
        Self::for_test(ProcessStatus::Draining, false, poison_lock)
    }

    #[cfg(test)]
    pub(crate) fn stopped_for_test() -> Self {
        Self::for_test(ProcessStatus::Stopped, true, false)
    }

    #[cfg(test)]
    pub(crate) fn set_owner_for_test(&self, owner: bool) {
        lock_process(&self.process.shared).shutdown_owner = owner;
    }

    #[cfg(test)]
    pub(crate) fn set_waker_for_test(&self, waker: std::task::Waker) {
        let mut state = lock_process(&self.process.shared);
        assert!(state.shutdown_waker.is_none());
        state.shutdown_waker = Some(waker);
    }

    #[cfg(test)]
    fn for_test(status: ProcessStatus, thread: bool, poison_lock: bool) -> Self {
        let shared = Arc::new(ProcessShared {
            state: std::sync::Mutex::new(ProcessState {
                status,
                shutdown_owner: true,
                next_registration: 0,
                registrations: std::collections::BTreeMap::new(),
                supervisors: std::collections::BTreeMap::new(),
                cursor: None,
                supervisor_cursor: None,
                epoch: 0,
                shutdown_waker: None,
                poisoned: false,
            }),
            signal: std::sync::Condvar::new(),
            wake: std::sync::OnceLock::new(),
        });
        if poison_lock {
            let shared = Arc::clone(&shared);
            let _ = std::panic::catch_unwind(move || {
                let _guard = shared.state.lock().unwrap();
                panic!("poison process state for test");
            });
        }
        let thread = thread.then(|| std::thread::spawn(|| {}));
        Self::new(Arc::new(ProcessReaper {
            shared,
            thread: std::sync::Mutex::new(thread),
        }))
    }
}

impl Future for SubscriptionProcessShutdown {
    type Output = Result<(), SubscriptionWorkerRuntimeError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut replacement = Some(context.waker().clone());
        let (invalid, poisoned, stopped, previous) = {
            let mut state = lock_process(&self.process.shared);
            let invalid = !state.shutdown_owner || !self.owns_shutdown;
            let stopped = state.status == ProcessStatus::Stopped;
            let previous = if !invalid && !stopped {
                state
                    .shutdown_waker
                    .replace(replacement.take().expect("proposed shutdown waker"))
            } else {
                None
            };
            (invalid, state.poisoned, stopped, previous)
        };
        drop(previous);
        drop(replacement);
        if invalid {
            return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
        }
        if !stopped {
            return Poll::Pending;
        }
        debug_assert!(stopped);

        let thread = match self.process.thread.lock() {
            Ok(mut thread) => thread.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        #[cfg(test)]
        let had_thread = thread.is_some();
        let join_succeeded = thread.is_some_and(|thread| thread.join().is_ok());
        #[cfg(test)]
        if had_thread {
            super::record_reaper_join();
        }

        let previous = {
            let mut state = lock_process(&self.process.shared);
            state.status = ProcessStatus::Joined;
            state.shutdown_owner = false;
            let previous = state.shutdown_waker.take();
            state.epoch = state.epoch.wrapping_add(1);
            previous
        };
        drop(previous);
        self.owns_shutdown = false;

        if !join_succeeded {
            Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
        } else if poisoned {
            Poll::Ready(Err(SubscriptionWorkerRuntimeError::Poisoned))
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl Drop for SubscriptionProcessShutdown {
    fn drop(&mut self) {
        if !self.owns_shutdown {
            return;
        }
        let mut state = lock_process(&self.process.shared);
        let mut previous = None;
        if state.shutdown_owner && state.status != ProcessStatus::Joined {
            state.shutdown_owner = false;
            previous = state.shutdown_waker.take();
            state.epoch = state.epoch.wrapping_add(1);
        }
        self.owns_shutdown = false;
        drop(state);
        drop(previous);
        self.process.shared.signal.notify_one();
    }
}
