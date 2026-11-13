//! Exact-generation emission owners for an active WebSocket subscription.

use super::ActiveWebSocketSubscription;
use crate::subscription::bridge_resources::{
    BridgeEmissionLease, BridgeResourceError, WebSocketOperationPhysicalLease,
};

impl ActiveWebSocketSubscription {
    /// Shares only the existing active charge with one immutable frame owner.
    pub(crate) fn physical_lease(
        &self,
    ) -> Result<WebSocketOperationPhysicalLease, BridgeResourceError> {
        self.operation
            .as_ref()
            .ok_or(BridgeResourceError::Invariant)?
            .websocket_physical_lease()
    }

    /// Clones only the exact-generation emission authority.
    pub(crate) fn emission_lease(&self) -> Result<BridgeEmissionLease, BridgeResourceError> {
        Ok(self
            .operation
            .as_ref()
            .ok_or(BridgeResourceError::Invariant)?
            .emission_lease())
    }
}
