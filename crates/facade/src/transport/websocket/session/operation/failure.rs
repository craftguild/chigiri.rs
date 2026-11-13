//! Credential-free failures that require the whole socket session to stop.

use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::control::SubscriptionControlSignal;
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::subscription::worker::SubscriptionWorkerRuntimeError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::transport::websocket::session) enum WebSocketOperationSharedFailure {
    Worker(SubscriptionWorkerRuntimeError),
    Resource(BridgeResourceError),
    Delivery(SubscriptionOutboxSignal),
    Control(SubscriptionControlSignal),
}

impl From<SubscriptionWorkerRuntimeError> for WebSocketOperationSharedFailure {
    fn from(error: SubscriptionWorkerRuntimeError) -> Self {
        Self::Worker(error)
    }
}

impl From<BridgeResourceError> for WebSocketOperationSharedFailure {
    fn from(error: BridgeResourceError) -> Self {
        Self::Resource(error)
    }
}

impl From<SubscriptionControlSignal> for WebSocketOperationSharedFailure {
    fn from(error: SubscriptionControlSignal) -> Self {
        Self::Control(error)
    }
}

pub(super) const fn is_shared_delivery(error: SubscriptionOutboxSignal) -> bool {
    matches!(
        error,
        SubscriptionOutboxSignal::Cancelled
            | SubscriptionOutboxSignal::NonMonotonicTime
            | SubscriptionOutboxSignal::InvalidTransportArguments
            | SubscriptionOutboxSignal::Invariant
            | SubscriptionOutboxSignal::Accounting
            | SubscriptionOutboxSignal::SendStalled
    )
}
