//! Opaque sharing of one existing active WebSocket operation charge.

use super::super::delivery::{OwnerGeneration, SharedActiveConnectionPermit};
use super::{BridgeGeneration, BridgeResourceError};

/// Physical ownership of an existing Query, Mutation, or Subscription operation.
///
/// Immutable socket frames and cleanup share the same original charge. Cloning
/// acquires no quota and grants no wire-ID, admission, or emission authority.
#[derive(Clone)]
pub struct WebSocketOperationPhysicalLease {
    permit: SharedActiveConnectionPermit,
}

impl WebSocketOperationPhysicalLease {
    pub(super) fn issue(
        permit: SharedActiveConnectionPermit,
        generation: OwnerGeneration,
    ) -> Result<Self, BridgeResourceError> {
        if permit.generation() != generation {
            return Err(BridgeResourceError::Invariant);
        }
        Ok(Self { permit })
    }

    /// Returns the exact private operation generation whose quota remains held.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.permit.generation())
    }
}
