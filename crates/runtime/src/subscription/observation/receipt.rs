//! Fixed identity and two coalesced wakers, with no log emission capability.

use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[derive(Default)]
struct State {
    accepted: bool,
    closed: bool,
    initializer: Option<Waker>,
    outside: Option<Waker>,
}

/// An unforgeable transfer identity, not a generation, sink, or emission owner.
///
/// ```compile_fail
/// use chigiri_runtime::__private::SubscriptionObservationReceipt;
/// let _ = SubscriptionObservationReceipt::new();
/// ```
#[derive(Clone)]
#[doc(hidden)]
pub struct SubscriptionObservationReceipt(Arc<Mutex<State>>);

impl SubscriptionObservationReceipt {
    pub(super) fn new() -> Self {
        Self(Arc::default())
    }

    pub(super) const fn retained_bytes() -> usize {
        // Include the Arc allocation header and its fixed state, not only the
        // pointer. Host-specific state is charged separately before allocation.
        std::mem::size_of::<Mutex<State>>() + 2 * std::mem::size_of::<usize>()
    }

    /// True only for clones of the exact same preparation's identity.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Closed ownership prevents delayed entry and registration.
    pub fn is_closed(&self) -> bool {
        self.0.lock().map_or(true, |state| state.closed)
    }

    /// Waits for the outside owner's acknowledgement, never for log delivery.
    pub async fn wait_for_acceptance(&self) -> bool {
        poll_fn(|context| {
            let mut proposed = Some(context.waker().clone());
            let mut state = match self.0.lock() {
                Ok(state) => state,
                Err(_) => return Poll::Ready(false),
            };
            let (result, previous) = if state.closed {
                (Poll::Ready(false), None)
            } else if state.accepted {
                (Poll::Ready(true), None)
            } else {
                (
                    Poll::Pending,
                    state.initializer.replace(proposed.take().unwrap()),
                )
            };
            drop(state);
            drop(previous);
            drop(proposed);
            result
        })
        .await
    }

    /// Coalesces host decision changes into the existing outside driver's poll.
    pub fn notify(&self) {
        let wake = self
            .0
            .lock()
            .ok()
            .and_then(|mut state| state.outside.take());
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    pub(super) fn accept(&self) -> bool {
        let (accepted, wake) = match self.0.lock() {
            Ok(mut state) if !state.closed => {
                state.accepted = true;
                (true, state.initializer.take())
            }
            _ => (false, None),
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        accepted
    }

    pub(super) fn arm_owner(&self, context: &mut Context<'_>) {
        let mut proposed = Some(context.waker().clone());
        let previous = self.0.lock().ok().and_then(|mut state| {
            if state.closed {
                None
            } else {
                state.outside.replace(proposed.take().unwrap())
            }
        });
        drop(previous);
        drop(proposed);
    }

    pub(super) fn close(&self) {
        let (initializer, outside) = match self.0.lock() {
            Ok(mut state) => {
                state.closed = true;
                (state.initializer.take(), state.outside.take())
            }
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.closed = true;
                (state.initializer.take(), state.outside.take())
            }
        };
        for wake in [initializer, outside].into_iter().flatten() {
            wake.wake();
        }
    }
}
