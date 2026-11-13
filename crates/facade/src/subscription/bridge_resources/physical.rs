//! Opaque physical ownership shared by HTTP source cleanup and wire output.

#[cfg(feature = "subscription-worker")]
use super::super::delivery::SharedConnectionPhysicalLease;
use super::super::delivery::{OwnerGeneration, SharedActiveSubscriptionPermit};
use super::admission::BridgeGeneration;
use super::error::BridgeResourceError;

/// Cloneable ownership of one existing HTTP active-subscription charge.
///
/// Chigiri issues this lease only from an exact active HTTP generation. Clones
/// share the original permit; they never acquire additional quota capacity.
#[derive(Clone)]
pub struct HttpSubscriptionPhysicalLease {
    permit: SharedActiveSubscriptionPermit,
}

impl HttpSubscriptionPhysicalLease {
    pub(super) fn issue(
        permit: &SharedActiveSubscriptionPermit,
        generation: OwnerGeneration,
    ) -> Result<Self, BridgeResourceError> {
        if permit.generation() != generation {
            return Err(BridgeResourceError::Invariant);
        }
        Ok(Self {
            permit: permit.clone(),
        })
    }

    /// Returns the exact endpoint-issued generation retained by this lease.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.permit.generation())
    }
}

/// Cloneable ownership of one existing physical WebSocket connection charge.
///
/// The lease can be issued only while the exact connection generation remains
/// logically open. Clones retain the original permit and acquire no capacity.
#[derive(Clone)]
pub struct WebSocketConnectionPhysicalLease {
    #[cfg(feature = "subscription-worker")]
    _permit: SharedConnectionPhysicalLease,
}

impl WebSocketConnectionPhysicalLease {
    #[cfg(feature = "subscription-worker")]
    pub(super) fn issue(permit: SharedConnectionPhysicalLease) -> Self {
        Self { _permit: permit }
    }

    /// Returns the exact endpoint-issued connection generation.
    #[cfg(feature = "subscription-worker")]
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self._permit.generation())
    }
}
