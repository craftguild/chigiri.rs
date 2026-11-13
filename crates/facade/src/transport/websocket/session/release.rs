//! Payload-free notification after the last immutable wire owner is destroyed.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::WebSocketIoError;

#[derive(Default)]
struct ReleaseState {
    released: bool,
    failed: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
pub(super) struct WireReleaseGate {
    state: Mutex<ReleaseState>,
}

impl WireReleaseGate {
    /// Called only after dropping the adapter's last physical payload owner.
    ///
    /// An event controller may separately retain its charged in-flight entry
    /// until finish_send; the waiter neither owns nor releases that entry.
    pub(super) fn release(&self) {
        let wake = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => {
                    let mut state = poisoned.into_inner();
                    state.failed = true;
                    state
                }
            };
            state.released = true;
            state.waker.take()
        };
        // A wake may reenter application code; no adapter payload or lock remains.
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

/// No bytes, quota, logical operation, or emission permission are retained here.
pub(super) struct WebSocketWireRelease {
    gate: Arc<WireReleaseGate>,
}

impl WebSocketWireRelease {
    pub(super) fn new(gate: &Arc<WireReleaseGate>) -> Self {
        Self {
            gate: Arc::clone(gate),
        }
    }

    pub(super) fn poll(&self, context: &mut Context<'_>) -> Poll<Result<(), WebSocketIoError>> {
        let Ok(mut state) = self.gate.state.lock() else {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        };
        if state.failed {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        }
        if state.released {
            return Poll::Ready(Ok(()));
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|waker| !waker.will_wake(context.waker()))
        {
            state.waker = Some(context.waker().clone());
        }
        Poll::Pending
    }
}
