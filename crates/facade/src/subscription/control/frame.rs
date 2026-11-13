//! Immutable control bytes with exact physical-lifetime charging.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chigiri_http::EncodedSubscriptionControl;

use super::super::delivery::InFlightByteCharge;
use super::websocket::WebSocketControlOperation;
#[cfg(any(test, feature = "subscription-worker"))]
use crate::subscription::bridge_resources::BridgeGeneration;

#[derive(Debug, Default)]
pub(super) struct InFlightGate {
    busy: AtomicBool,
}

impl InFlightGate {
    pub(super) fn try_acquire(self: &Arc<Self>) -> Option<InFlightLease> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| InFlightLease {
                gate: Arc::clone(self),
            })
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

pub(super) struct InFlightLease {
    gate: Arc<InFlightGate>,
}

impl Drop for InFlightLease {
    fn drop(&mut self) {
        self.gate.busy.store(false, Ordering::Release);
    }
}

/// A move-only immutable control frame for zero-copy bridge ownership.
///
/// The exact endpoint byte charge and the staging in-flight gate remain held
/// until this value and every shared physical owner containing it are dropped.
pub struct SubscriptionControlFrame {
    encoded: EncodedSubscriptionControl,
    _charge: InFlightByteCharge,
    _lease: InFlightLease,
    operation: Option<WebSocketControlOperation>,
}

impl SubscriptionControlFrame {
    pub(super) fn new(
        encoded: EncodedSubscriptionControl,
        charge: InFlightByteCharge,
        lease: InFlightLease,
    ) -> Self {
        Self {
            encoded,
            _charge: charge,
            _lease: lease,
            operation: None,
        }
    }

    pub(crate) fn with_websocket_operation(
        mut self,
        operation: Option<WebSocketControlOperation>,
    ) -> Self {
        self.operation = operation;
        self
    }

    /// Returns an internal generation fence, never an emission authorization.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn websocket_generation(&self) -> Option<BridgeGeneration> {
        self.operation
            .as_ref()
            .and_then(WebSocketControlOperation::generation)
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn websocket_operation_id(&self) -> Option<&str> {
        self.operation.as_ref().map(WebSocketControlOperation::id)
    }

    /// Returns the exact framed byte length.
    pub fn len(&self) -> usize {
        self.encoded.len()
    }

    /// Returns whether the frame occupies no bytes.
    pub fn is_empty(&self) -> bool {
        self.encoded.is_empty()
    }

    /// Borrows immutable bytes for a body or socket write.
    pub fn as_bytes(&self) -> &[u8] {
        self.encoded.as_bytes()
    }
}

impl AsRef<[u8]> for SubscriptionControlFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for SubscriptionControlFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionControlFrame")
            .field("bytes", &self.len())
            .finish_non_exhaustive()
    }
}
