//! Move-only WebSocket connection ownership and weak admission views.

use std::sync::Arc;

use super::super::delivery::{
    CleanupTicket, ConnectionAdmissionResources, ConnectionResources, OwnerGeneration,
    QuarantineRegistry,
};
use super::BridgeGeneration;
use super::admission::{PendingBridgeOperation, reserve_ticket};
use super::cleanup::TerminalBridgeConnection;
use super::error::BridgeResourceError;

/// One endpoint-charged, move-only WebSocket connection owner.
///
/// The connection owner cannot be cloned; only [`BridgeConnectionAdmission`]
/// is cloneable.
///
/// ```compile_fail
/// use chigiri::__streaming::BridgeConnection;
///
/// fn duplicate(owner: BridgeConnection) {
///     let _second: BridgeConnection = owner.clone();
/// }
/// ```
pub struct BridgeConnection {
    physical: Option<ConnectionResources>,
    admission: BridgeConnectionAdmission,
    generation: OwnerGeneration,
    ticket: Option<CleanupTicket>,
    #[cfg(feature = "subscription-worker")]
    socket_scope: Option<std::sync::Weak<crate::managed::SocketScope>>,
}

impl BridgeConnection {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn bind_socket_scope(&mut self, scope: &Arc<crate::managed::SocketScope>) {
        self.socket_scope = Some(Arc::downgrade(scope));
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn socket_activity(&self) -> Option<crate::managed::EventActivity> {
        self.socket_scope
            .as_ref()?
            .upgrade()
            .map(|scope| scope.activity())
    }

    pub(super) fn new(
        physical: ConnectionResources,
        quarantine: QuarantineRegistry,
    ) -> Result<Self, BridgeResourceError> {
        let generation = physical.generation();
        let ticket = reserve_ticket(&quarantine, generation)?;
        let admission = BridgeConnectionAdmission {
            resources: physical.admission(),
            quarantine,
        };
        Ok(Self {
            physical: Some(physical),
            admission,
            generation,
            ticket: Some(ticket),
            #[cfg(feature = "subscription-worker")]
            socket_scope: None,
        })
    }

    /// Returns this connection exact endpoint-issued generation.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
    }

    /// Returns a weak cloneable view for operation admission.
    pub fn admission(&self) -> BridgeConnectionAdmission {
        self.admission.clone()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn physical_lease(
        &self,
    ) -> Result<super::WebSocketConnectionPhysicalLease, BridgeResourceError> {
        let physical = self
            .physical
            .as_ref()
            .ok_or(BridgeResourceError::Invariant)?;
        let permit = physical.physical_lease()?;
        if permit.generation() != self.generation {
            return Err(BridgeResourceError::Invariant);
        }
        Ok(super::WebSocketConnectionPhysicalLease::issue(permit))
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn spawn_worker<F>(
        &self,
        future: F,
    ) -> Result<crate::subscription::worker::ConcreteWorker<F::Output>, BridgeResourceError>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.ticket
            .as_ref()
            .ok_or(BridgeResourceError::Invariant)?
            .spawn_worker(future)
            .map_err(|_| BridgeResourceError::Invariant)
    }

    /// Acquires pending quotas while this owner remains logically open.
    pub fn begin(
        &self,
        logical_id: Arc<str>,
    ) -> Result<PendingBridgeOperation, BridgeResourceError> {
        self.admission.begin(logical_id)
    }

    /// Closes every admission view and retains the physical connection owner.
    pub fn revoke(mut self) -> Result<TerminalBridgeConnection, BridgeResourceError> {
        let physical = self.physical.take().ok_or(BridgeResourceError::Invariant)?;
        let ticket = self.ticket.take().ok_or(BridgeResourceError::Invariant)?;
        let terminal = TerminalBridgeConnection::new(self.generation, physical.revoke(), ticket);
        #[cfg(feature = "subscription-worker")]
        let terminal = {
            let mut terminal = terminal;
            terminal.bind_socket_scope(self.socket_scope.take());
            terminal
        };
        Ok(terminal)
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> super::super::delivery::QuotaSnapshot {
        self.physical
            .as_ref()
            .expect("live bridge connection")
            .snapshot()
    }
}

#[cfg(feature = "subscription-worker")]
impl Drop for BridgeConnection {
    fn drop(&mut self) {
        let Some(scope) = self.socket_scope.take().and_then(|scope| scope.upgrade()) else {
            return;
        };
        if let (Some(physical), Some(ticket)) = (self.physical.take(), self.ticket.take()) {
            scope.retain_cleanup(super::cleanup::TerminalCleanupOwner::new(
                ticket,
                super::super::delivery::CleanupPhysicalCharge::ConnectionOwner {
                    _permit: physical.revoke(),
                },
            ));
        }
    }
}

/// Cloneable operation-admission view that cannot retain the connection permit.
#[derive(Clone)]
pub struct BridgeConnectionAdmission {
    resources: ConnectionAdmissionResources,
    quarantine: QuarantineRegistry,
}

impl BridgeConnectionAdmission {
    /// Acquires endpoint and connection pending quotas with one logical ID.
    pub fn begin(
        &self,
        logical_id: Arc<str>,
    ) -> Result<PendingBridgeOperation, BridgeResourceError> {
        let pending = self.resources.begin(Arc::clone(&logical_id))?;
        let generation = pending.generation();
        let ticket = reserve_ticket(&self.quarantine, generation)?;
        Ok(PendingBridgeOperation::from_connection(pending, ticket))
    }
}
