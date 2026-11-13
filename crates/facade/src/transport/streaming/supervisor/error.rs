//! Closed failures shared by the official HTTP streaming adapters.

use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::control::SubscriptionControlSignal;
use crate::subscription::outbox::SubscriptionOutboxSignal;

/// A credential-free streaming infrastructure or delivery failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpStreamingError {
    /// Endpoint admission or explicit runtime registration failed.
    Resource(BridgeResourceError),
    /// The application closed supervision or the response body.
    Closed,
    /// Bounded supervision metadata could not be allocated or admitted.
    Capacity,
    /// Exact-generation ownership was not preserved.
    Invariant,
    /// The monotonic epoch overflowed or moved backwards.
    Clock,
    /// A concrete source worker failed, rather than naturally completing.
    Worker,
    /// An actual application deadline expired.
    TimedOut,
    /// The common event delivery controller terminated this stream.
    Delivery(SubscriptionOutboxSignal),
    /// Bounded control staging failed.
    Control(SubscriptionControlSignal),
}

impl std::fmt::Display for HttpStreamingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HTTP subscription streaming failed")
    }
}

impl std::error::Error for HttpStreamingError {}
