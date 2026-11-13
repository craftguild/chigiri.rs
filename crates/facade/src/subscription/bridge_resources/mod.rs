//! Opaque resource ownership used only by official streaming bridges.

mod admission;
mod cleanup;
mod connection;
mod coordinator;
mod error;
mod operation_physical;
mod physical;
#[cfg(feature = "subscription-worker")]
pub(crate) use cleanup::TerminalCleanupOwner;

pub(crate) use admission::BridgeResourceOwner;
pub use admission::{
    ActiveBridgeOperation, BridgeGeneration, BridgeOperationKind, PendingBridgeOperation,
};
pub use cleanup::{
    BridgeCancellation, BridgeCleanupDriver, BridgeEmissionLease, BridgeQuarantineFailure,
    DetachedCleanup, TerminalBridgeConnection, TerminalBridgeOperation,
};
pub use connection::{BridgeConnection, BridgeConnectionAdmission};
pub use coordinator::{BridgeCleanupCoordinator, BridgeCleanupOutcome};
pub use error::{BridgeCleanupError, BridgeResourceError};
pub use operation_physical::WebSocketOperationPhysicalLease;
pub use physical::{HttpSubscriptionPhysicalLease, WebSocketConnectionPhysicalLease};

#[cfg(test)]
mod tests;
