//! Library-neutral socket operations and immutable physical payload ownership.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::super::{
    WebSocketAcknowledgementFrame, WebSocketConnectionClose, WebSocketConnectionFrame,
};
use super::release::{WebSocketWireRelease, WireReleaseGate};
use crate::subscription::bridge_resources::WebSocketOperationPhysicalLease;
use crate::subscription::outbox::SubscriptionSendFrame;

/// One complete bounded application message, not an RFC control frame.
#[doc(hidden)]
pub enum WebSocketInbound {
    /// Complete UTF-8 text; the adapter enforces the frozen one-MiB ceiling.
    Text(String),
    /// A binary application message, without retaining its rejected payload.
    Binary,
}

/// Closed safe I/O classifications with no raw library error or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum WebSocketIoError {
    /// The underlying socket or its writer failed.
    Transport,
    /// Invalid RFC framing, UTF-8, or an oversized inbound application message.
    Protocol,
    /// An official adapter violated its single-writer ownership contract.
    Invariant,
}

/// Positive accepted payload bytes and an optional successful frame flush.
///
/// Counts are deltas for the sole current application frame. RFC headers,
/// automatic ping/pong/close bytes, and other operations never contribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct WebSocketWriteProgress {
    /// Newly accepted application payload bytes, not the cumulative count.
    pub accepted: usize,
    /// The whole current frame was successfully flushed by the socket writer.
    pub flushed: bool,
}

enum WirePayload {
    Acknowledgement(WebSocketAcknowledgementFrame),
    Control(WebSocketConnectionFrame),
    Event {
        frame: SubscriptionSendFrame,
        _physical: WebSocketOperationPhysicalLease,
    },
}

struct WireOwner {
    payload: Option<WirePayload>,
    release: Arc<WireReleaseGate>,
}

impl Drop for WireOwner {
    fn drop(&mut self) {
        // Destroy this physical payload reference before notifying its driver.
        // A common controller may retain its independent in-flight entry.
        drop(self.payload.take());
        self.release.release();
    }
}

/// Immutable zero-copy payload retaining its original bytes and physical quota.
///
/// A socket adapter may clone this opaque owner, but must retain it through
/// write/flush and every physical buffer reference. Cloning grants no execution,
/// admission, or emission authority. Last Drop releases the original ownership.
#[derive(Clone)]
#[doc(hidden)]
pub struct WebSocketWireFrame {
    owner: Arc<WireOwner>,
}

impl WebSocketWireFrame {
    pub(super) fn acknowledgement(frame: WebSocketAcknowledgementFrame) -> Self {
        Self::new(WirePayload::Acknowledgement(frame))
    }

    pub(super) fn control(frame: WebSocketConnectionFrame) -> Self {
        Self::new(WirePayload::Control(frame))
    }

    pub(super) fn event(
        frame: SubscriptionSendFrame,
        physical: WebSocketOperationPhysicalLease,
    ) -> Self {
        Self::new(WirePayload::Event {
            frame,
            _physical: physical,
        })
    }

    fn new(payload: WirePayload) -> Self {
        Self {
            owner: Arc::new(WireOwner {
                payload: Some(payload),
                release: Arc::new(WireReleaseGate::default()),
            }),
        }
    }

    pub(super) fn release(&self) -> WebSocketWireRelease {
        WebSocketWireRelease::new(&self.owner.release)
    }

    /// Borrows the original encoded bytes without allocation or JSON conversion.
    pub fn as_bytes(&self) -> &[u8] {
        match self.owner.payload.as_ref().expect("live wire payload") {
            WirePayload::Acknowledgement(frame) => frame.as_bytes(),
            WirePayload::Control(frame) => frame.as_bytes(),
            WirePayload::Event { frame, .. } => frame.as_bytes(),
        }
    }
}

impl AsRef<[u8]> for WebSocketWireFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for WebSocketWireFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketWireFrame")
            .field("bytes", &self.as_bytes().len())
            .finish_non_exhaustive()
    }
}

/// Poll-based socket port implemented only by the official protocol adapters.
///
/// The application owns and polls the connection task. There is at most one
/// frame between `start_send` and a successful flush. The adapter retains its
/// opaque owner through every physical reference, even on cancellation/error.
/// Receive enforces the one-MiB message ceiling before allocation can exceed it;
/// RFC control handling must not queue unbounded work or count as event progress.
#[doc(hidden)]
pub trait WebSocketIo: Send + Unpin + 'static {
    /// Receives one complete application message, or `None` at peer shutdown.
    fn poll_receive(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>>;

    /// Waits for the sole whole-frame writer to accept one immutable payload.
    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>>;

    /// Transfers the original physical payload owner, without a second queue.
    fn start_send(self: Pin<&mut Self>, frame: WebSocketWireFrame) -> Result<(), WebSocketIoError>;

    /// Reports current-frame payload progress or successful flush exactly once.
    ///
    /// Return Pending if neither happened. A flushed result must account for
    /// every payload byte; a subsequent frame cannot begin until that result.
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>>;

    /// Attempts bounded shutdown; `None` makes no promise of a final close frame.
    fn poll_close(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>>;
}
