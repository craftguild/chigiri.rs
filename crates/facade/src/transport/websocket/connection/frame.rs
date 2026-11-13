//! Physical connection ownership attached to immutable control frames.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::subscription::bridge_resources::WebSocketConnectionPhysicalLease;
use crate::subscription::control::SubscriptionControlFrame;

/// One immutable control frame retaining the physical WebSocket connection.
#[doc(hidden)]
pub struct WebSocketConnectionFrame {
    frame: SubscriptionControlFrame,
    _physical: WebSocketConnectionPhysicalLease,
}

impl WebSocketConnectionFrame {
    pub(in crate::transport::websocket) const fn new(
        frame: SubscriptionControlFrame,
        physical: WebSocketConnectionPhysicalLease,
    ) -> Self {
        Self {
            frame,
            _physical: physical,
        }
    }

    /// Returns the exact immutable encoded bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.frame.as_bytes()
    }
}

impl AsRef<[u8]> for WebSocketConnectionFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for WebSocketConnectionFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketConnectionFrame")
            .field("bytes", &self.as_bytes().len())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct AckState {
    frame_released: bool,
    flushed: bool,
    failed: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
pub(super) struct AckGate {
    state: Mutex<AckState>,
}

impl AckGate {
    pub(super) fn poll(&self, context: &mut Context<'_>) -> Poll<Result<(), ()>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.failed {
            return Poll::Ready(Err(()));
        }
        if state.frame_released && state.flushed {
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

    fn update(&self, change: impl FnOnce(&mut AckState)) {
        let wake = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            change(&mut state);
            state.waker.take()
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }

    fn frame_released(&self) {
        self.update(|state| state.frame_released = true);
    }

    fn flushed(&self) {
        self.update(|state| state.flushed = true);
    }

    fn failed(&self) {
        self.update(|state| state.failed = true);
    }
}

/// The sole acknowledgement frame, retaining its physical connection lease.
#[doc(hidden)]
pub struct WebSocketAcknowledgementFrame {
    frame: Option<WebSocketConnectionFrame>,
    gate: Arc<AckGate>,
}

impl WebSocketAcknowledgementFrame {
    pub(super) const fn new(frame: WebSocketConnectionFrame, gate: Arc<AckGate>) -> Self {
        Self {
            frame: Some(frame),
            gate,
        }
    }

    /// Returns the exact immutable acknowledgement bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.frame
            .as_ref()
            .expect("live acknowledgement frame")
            .as_bytes()
    }
}

impl AsRef<[u8]> for WebSocketAcknowledgementFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Drop for WebSocketAcknowledgementFrame {
    fn drop(&mut self) {
        drop(self.frame.take());
        self.gate.frame_released();
    }
}

/// One-shot writer proof that the acknowledgement was successfully flushed.
#[must_use = "a dropped acknowledgement progress token fails the connection"]
#[doc(hidden)]
pub struct WebSocketAcknowledgementProgress {
    gate: Arc<AckGate>,
    live: bool,
}

impl WebSocketAcknowledgementProgress {
    pub(super) const fn new(gate: Arc<AckGate>) -> Self {
        Self { gate, live: true }
    }

    /// Records successful physical writer flush and consumes this proof.
    pub fn written_and_flushed(mut self) {
        self.live = false;
        self.gate.flushed();
    }
}

impl Drop for WebSocketAcknowledgementProgress {
    fn drop(&mut self) {
        if self.live {
            self.live = false;
            self.gate.failed();
        }
    }
}
