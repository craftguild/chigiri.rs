//! Endpoint-issued HTTP and multiplexed connection admission handles.

use super::super::delivery::{
    ActiveConnectionOperation, CancellationFlag, CleanupPhysicalCharge, CleanupTicket,
    DeliveryResources, EmissionAuthority, OperationKind, OwnerGeneration,
    PendingConnectionOperation, PendingEndpointOperation, QuarantineError, QuarantineKey,
    QuarantineRegistry, SharedActiveSubscriptionPermit,
};
use super::super::settings::ValidatedSubscriptionSettings;
use super::cleanup::{BridgeCancellation, BridgeEmissionLease, TerminalBridgeOperation};
use super::error::BridgeResourceError;
use super::physical::HttpSubscriptionPhysicalLease;

/// Operation classes charged against one multiplexed connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeOperationKind {
    /// A finite read operation sharing the connection operation quota.
    Query,
    /// A finite mutation sharing the connection operation quota.
    Mutation,
    /// A streaming subscription that also charges the endpoint active limit.
    Subscription,
}

impl From<BridgeOperationKind> for OperationKind {
    fn from(kind: BridgeOperationKind) -> Self {
        match kind {
            BridgeOperationKind::Query => Self::Query,
            BridgeOperationKind::Mutation => Self::Mutation,
            BridgeOperationKind::Subscription => Self::Subscription,
        }
    }
}

/// Opaque endpoint-issued generation; callers cannot construct or reuse it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BridgeGeneration(pub(super) OwnerGeneration);

/// Root owner retained by one immutable built endpoint.
pub(crate) struct BridgeResourceOwner {
    resources: DeliveryResources,
}

impl BridgeResourceOwner {
    pub(crate) fn new(
        settings: &ValidatedSubscriptionSettings,
    ) -> Result<Self, BridgeResourceError> {
        Ok(Self {
            resources: DeliveryResources::new(settings)?,
        })
    }

    pub(crate) fn begin_http_subscription(
        &self,
    ) -> Result<PendingBridgeOperation, BridgeResourceError> {
        let pending = self.resources.try_pending_operation()?;
        let generation = pending.generation();
        let quarantine = self.resources.quarantine();
        let ticket = reserve_ticket(&quarantine, generation)?;
        Ok(PendingBridgeOperation {
            physical: Some(PendingPhysical::Http(pending)),
            generation,
            ticket: Some(ticket),
        })
    }

    pub(crate) fn open_connection(&self) -> Result<BridgeConnection, BridgeResourceError> {
        BridgeConnection::new(
            self.resources.open_connection()?,
            self.resources.quarantine(),
        )
    }

    pub(crate) fn cleanup_driver(&self) -> super::BridgeCleanupDriver {
        super::BridgeCleanupDriver::new(self.resources.quarantine())
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn register_worker_runtime(
        &self,
    ) -> Result<crate::subscription::worker::SubscriptionRuntimeGuard, BridgeResourceError> {
        self.resources.register_worker_runtime().map_err(Into::into)
    }

    pub(crate) fn delivery(&self) -> &DeliveryResources {
        &self.resources
    }
}

use super::connection::BridgeConnection;

pub(super) fn reserve_ticket(
    quarantine: &QuarantineRegistry,
    generation: OwnerGeneration,
) -> Result<CleanupTicket, BridgeResourceError> {
    quarantine
        .reserve(QuarantineKey::new(generation))
        .map_err(|error| match error {
            QuarantineError::Capacity => BridgeResourceError::Capacity,
            QuarantineError::DuplicateGeneration | QuarantineError::Invariant => {
                BridgeResourceError::Invariant
            }
        })
}

enum PendingPhysical {
    Http(PendingEndpointOperation),
    Connection(PendingConnectionOperation),
}

/// Move-only pending setup ownership for HTTP or multiplexed transports.
pub struct PendingBridgeOperation {
    physical: Option<PendingPhysical>,
    generation: OwnerGeneration,
    ticket: Option<CleanupTicket>,
}

impl PendingBridgeOperation {
    /// Returns the endpoint-issued generation without exposing its numeric value.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
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

    /// Atomically transfers pending ownership to the applicable active quotas.
    pub fn activate(
        mut self,
        kind: BridgeOperationKind,
    ) -> Result<ActiveBridgeOperation, BridgeResourceError> {
        let physical = self.physical.take().ok_or(BridgeResourceError::Invariant)?;
        let ticket = self.ticket.take().ok_or(BridgeResourceError::Invariant)?;
        let (generation, active) = match physical {
            PendingPhysical::Http(pending) => {
                if kind != BridgeOperationKind::Subscription {
                    return Err(BridgeResourceError::InvalidOperationKind);
                }
                (
                    self.generation,
                    ActivePhysical::Http(pending.activate_subscription()?),
                )
            }
            PendingPhysical::Connection(pending) => (
                self.generation,
                ActivePhysical::Connection(pending.activate(kind.into())?),
            ),
        };
        let cancellation = CancellationFlag::default();
        let emission = EmissionAuthority::new(generation);
        Ok(ActiveBridgeOperation {
            physical: Some(active),
            generation,
            cancellation,
            emission,
            ticket: Some(ticket),
        })
    }

    /// Releases any wire ID immediately while retaining pending physical quotas.
    pub fn revoke(mut self) -> Result<TerminalBridgeOperation, BridgeResourceError> {
        let physical = self.physical.take().ok_or(BridgeResourceError::Invariant)?;
        let ticket = self.ticket.take().ok_or(BridgeResourceError::Invariant)?;
        let charge = match physical {
            PendingPhysical::Http(pending) => CleanupPhysicalCharge::PendingHttp {
                _permit: pending.revoke(),
            },
            PendingPhysical::Connection(pending) => CleanupPhysicalCharge::PendingConnection {
                _permit: pending.revoke(),
            },
        };
        Ok(TerminalBridgeOperation::new(
            self.generation,
            charge,
            ticket,
        ))
    }

    pub(super) fn from_connection(
        pending: PendingConnectionOperation,
        ticket: CleanupTicket,
    ) -> Self {
        let generation = pending.generation();
        Self {
            physical: Some(PendingPhysical::Connection(pending)),
            generation,
            ticket: Some(ticket),
        }
    }
}

enum ActivePhysical {
    Http(SharedActiveSubscriptionPermit),
    Connection(ActiveConnectionOperation),
}

/// Exact-generation logical and physical ownership after activation.
pub struct ActiveBridgeOperation {
    physical: Option<ActivePhysical>,
    generation: OwnerGeneration,
    cancellation: CancellationFlag,
    emission: EmissionAuthority,
    ticket: Option<CleanupTicket>,
}

impl ActiveBridgeOperation {
    /// Returns the exact endpoint-issued generation owned by this operation.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
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

    /// Returns a cancellation observation that owns no endpoint resources.
    pub fn cancellation(&self) -> BridgeCancellation {
        BridgeCancellation::new(self.cancellation.clone())
    }

    /// Returns an exact-generation check for the final pre-emission fence.
    pub fn emission_lease(&self) -> BridgeEmissionLease {
        BridgeEmissionLease::new(self.emission.clone(), self.generation)
    }

    /// Issues shared physical ownership only for this exact HTTP generation.
    pub fn http_physical_lease(
        &self,
    ) -> Result<HttpSubscriptionPhysicalLease, BridgeResourceError> {
        match self.physical.as_ref() {
            Some(ActivePhysical::Http(permit)) => {
                HttpSubscriptionPhysicalLease::issue(permit, self.generation)
            }
            Some(ActivePhysical::Connection(_)) => Err(BridgeResourceError::InvalidOperationKind),
            None => Err(BridgeResourceError::Invariant),
        }
    }

    /// Issues only this exact WebSocket operation's original physical charge.
    pub fn websocket_physical_lease(
        &self,
    ) -> Result<super::WebSocketOperationPhysicalLease, BridgeResourceError> {
        match self.physical.as_ref() {
            Some(ActivePhysical::Connection(operation)) => {
                super::WebSocketOperationPhysicalLease::issue(
                    operation.physical_lease()?,
                    self.generation,
                )
            }
            Some(ActivePhysical::Http(_)) => Err(BridgeResourceError::InvalidOperationKind),
            None => Err(BridgeResourceError::Invariant),
        }
    }

    /// Revokes emission and the logical ID while retaining physical charges.
    pub fn revoke(mut self) -> Result<TerminalBridgeOperation, BridgeResourceError> {
        self.revoke_signals();
        let physical = self.physical.take().ok_or(BridgeResourceError::Invariant)?;
        let ticket = self.ticket.take().ok_or(BridgeResourceError::Invariant)?;
        let charge = match physical {
            ActivePhysical::Http(permit) => CleanupPhysicalCharge::Http { _permit: permit },
            ActivePhysical::Connection(operation) => {
                let revoked = operation.revoke();
                CleanupPhysicalCharge::Connection { _permit: revoked }
            }
        };
        Ok(TerminalBridgeOperation::new(
            self.generation,
            charge,
            ticket,
        ))
    }

    fn revoke_signals(&self) {
        self.emission.revoke();
        self.cancellation.cancel();
    }
}

impl Drop for ActiveBridgeOperation {
    fn drop(&mut self) {
        self.revoke_signals();
    }
}
