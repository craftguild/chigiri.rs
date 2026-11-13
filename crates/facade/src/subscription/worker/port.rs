//! Fixed-capacity single-value ports between an outer owner and its worker.

use std::future::poll_fn;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use super::SubscriptionWorkerRuntimeError;

pub(super) fn channel<T>() -> (PortSender<T>, PortReceiver<T>) {
    let shared = Arc::new(PortShared {
        state: Mutex::new(PortState {
            value: None,
            closed: false,
            receiver_waker: None,
        }),
    });
    (
        PortSender {
            shared: Arc::clone(&shared),
        },
        PortReceiver { shared },
    )
}

struct PortShared<T> {
    state: Mutex<PortState<T>>,
}

struct PortState<T> {
    value: Option<T>,
    closed: bool,
    receiver_waker: Option<Waker>,
}

impl<T> PortShared<T> {
    fn lock(&self) -> Result<MutexGuard<'_, PortState<T>>, SubscriptionWorkerRuntimeError> {
        self.state
            .lock()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)
    }

    fn close_sender(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.closed = true;
        let receiver = state.receiver_waker.take();
        drop(state);
        if let Some(waker) = receiver {
            waker.wake();
        }
    }

    fn close_receiver(&self) -> Result<Option<T>, SubscriptionWorkerRuntimeError> {
        let (receiver, value, poisoned) = {
            let (mut state, poisoned) = match self.state.lock() {
                Ok(state) => (state, false),
                Err(poisoned) => (poisoned.into_inner(), true),
            };
            state.closed = true;
            (state.receiver_waker.take(), state.value.take(), poisoned)
        };
        drop(receiver);
        if poisoned {
            drop(value);
            Err(SubscriptionWorkerRuntimeError::Invariant)
        } else {
            Ok(value)
        }
    }
}

pub(crate) struct PortSender<T> {
    shared: Arc<PortShared<T>>,
}

impl<T> PortSender<T> {
    pub(super) fn try_send(&self, value: T) -> Result<(), T> {
        if let Some(receiver) = self.try_send_deferred(value)? {
            receiver.wake();
        }
        Ok(())
    }

    pub(super) fn try_send_deferred(&self, value: T) -> Result<Option<Waker>, T> {
        let mut state = match self.shared.lock() {
            Ok(state) => state,
            Err(_) => return Err(value),
        };
        if state.closed || state.value.is_some() {
            return Err(value);
        }
        state.value = Some(value);
        let receiver = state.receiver_waker.take();
        drop(state);
        Ok(receiver)
    }

    pub(super) fn close(&self) {
        self.shared.close_sender();
    }
}

impl<T> Drop for PortSender<T> {
    fn drop(&mut self) {
        self.shared.close_sender();
    }
}

pub(crate) struct PortReceiver<T> {
    shared: Arc<PortShared<T>>,
}

impl<T> PortReceiver<T> {
    pub(super) fn try_take(&mut self) -> Result<Option<T>, SubscriptionWorkerRuntimeError> {
        Ok(self.shared.lock()?.value.take())
    }

    /// Observes sender closure without consuming a queued demand or result.
    pub(super) fn poll_closed(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        let mut replacement = Some(context.waker().clone());
        let mut state = match self.shared.lock() {
            Ok(state) => state,
            Err(error) => {
                drop(replacement);
                return Poll::Ready(Err(error));
            }
        };
        let (result, previous) = if state.closed {
            (Poll::Ready(Ok(())), None)
        } else {
            let previous = state
                .receiver_waker
                .replace(replacement.take().expect("proposed receiver waker"));
            (Poll::Pending, previous)
        };
        drop(state);
        drop(previous);
        drop(replacement);
        result
    }

    pub(super) fn poll_take(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<T>, SubscriptionWorkerRuntimeError>> {
        let mut replacement = Some(context.waker().clone());
        let mut state = match self.shared.lock() {
            Ok(state) => state,
            Err(error) => {
                drop(replacement);
                return Poll::Ready(Err(error));
            }
        };
        // A ready receive may be followed by the next source demand in the
        // same aggregate-driver turn. Keep its notification armed even when
        // this poll takes a value, so the next send cannot lose that wake.
        let previous = if state.closed {
            None
        } else {
            state
                .receiver_waker
                .replace(replacement.take().expect("proposed receiver waker"))
        };
        let result = if let Some(value) = state.value.take() {
            Poll::Ready(Ok(Some(value)))
        } else if state.closed {
            Poll::Ready(Ok(None))
        } else {
            Poll::Pending
        };
        drop(state);
        drop(previous);
        drop(replacement);
        result
    }

    pub(super) async fn receive(&mut self) -> Result<Option<T>, SubscriptionWorkerRuntimeError> {
        poll_fn(|context| self.poll_take(context)).await
    }

    /// Close the result port and transfer its last value without discarding it.
    pub(super) fn close_and_take(&self) -> Result<Option<T>, SubscriptionWorkerRuntimeError> {
        self.shared.close_receiver()
    }

    pub(super) fn close(&self) {
        drop(self.shared.close_receiver());
    }
}

#[cfg(test)]
impl<T: Send + 'static> PortReceiver<T> {
    pub(crate) fn lock_probe_for_test(&self) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        let shared = Arc::downgrade(&self.shared);
        Arc::new(move || {
            shared.upgrade().is_some_and(|shared| {
                matches!(
                    shared.state.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                )
            })
        })
    }

    pub(crate) fn poison_for_test(&self) {
        let shared = Arc::clone(&self.shared);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = shared.state.lock().unwrap();
            panic!("poison subscription worker port for test");
        }));
    }
}

impl<T> Drop for PortReceiver<T> {
    fn drop(&mut self) {
        drop(self.shared.close_receiver());
    }
}
