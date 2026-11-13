//! Endpoint-wide admission and construction of independent connection owners.

use std::sync::{Arc, Mutex};

use crate::subscription::settings::ValidatedSubscriptionSettings;

#[cfg(test)]
use super::QuotaSnapshot;
use super::{
    ConnectionInner, ConnectionResources, ConnectionState, EndpointQuotaOwner, EndpointQuotaState,
    OpenConnectionLease, OwnerGeneration, PermitError, next_generation, release_endpoint_active,
    release_endpoint_pending,
};
use crate::subscription::delivery::{EndpointByteOwner, QuarantineRegistry};

/// Shared endpoint quota, byte, and quarantine owners.
#[derive(Clone)]
pub(crate) struct DeliveryResources {
    quota: EndpointQuotaOwner,
    bytes: EndpointByteOwner,
    quarantine: QuarantineRegistry,
    connection_limits: ConnectionLimits,
}

#[derive(Clone, Copy)]
struct ConnectionLimits {
    max_pending: usize,
    max_active: usize,
}

impl DeliveryResources {
    pub(crate) fn new(settings: &ValidatedSubscriptionSettings) -> Result<Self, PermitError> {
        let limits = settings.limits();
        let protocol = settings.protocol();
        let max_quarantine_entries = protocol
            .max_websocket_connections
            .checked_mul(protocol.max_active_operations_per_connection)
            .and_then(|value| value.checked_add(limits.max_active_subscriptions))
            .and_then(|value| value.checked_add(limits.max_pending_operation_setups))
            .and_then(|value| value.checked_add(protocol.max_websocket_connections))
            .ok_or(PermitError::Invariant)?;
        let quarantine = QuarantineRegistry::new(max_quarantine_entries);
        Ok(Self {
            quota: EndpointQuotaOwner {
                inner: Arc::new(Mutex::new(EndpointQuotaState {
                    max_pending: limits.max_pending_operation_setups,
                    max_active_subscriptions: limits.max_active_subscriptions,
                    max_connections: protocol.max_websocket_connections,
                    pending: 0,
                    active_subscriptions: 0,
                    connections: 0,
                    last_generation: 0,
                    corrupted: false,
                })),
            },
            bytes: EndpointByteOwner::new(limits.max_endpoint_encoded_bytes),
            quarantine,
            connection_limits: ConnectionLimits {
                max_pending: protocol.max_pending_setups_per_connection,
                max_active: protocol.max_active_operations_per_connection,
            },
        })
    }

    pub(crate) fn byte_owner(&self) -> EndpointByteOwner {
        self.bytes.clone()
    }

    pub(crate) fn quarantine(&self) -> QuarantineRegistry {
        self.quarantine.clone()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn register_worker_runtime(
        &self,
    ) -> Result<crate::subscription::worker::SubscriptionRuntimeGuard, PermitError> {
        self.quarantine
            .register_worker_runtime()
            .map_err(|_| PermitError::Invariant)
    }

    pub(crate) fn try_pending_operation(&self) -> Result<PendingEndpointOperation, PermitError> {
        let mut endpoint = self.quota.lock()?;
        if endpoint.pending >= endpoint.max_pending {
            return Err(PermitError::Capacity);
        }
        let generation = next_generation(&mut endpoint)?;
        endpoint.pending += 1;
        Ok(PendingEndpointOperation {
            endpoint: self.quota.clone(),
            generation,
            live: true,
        })
    }

    pub(crate) fn open_connection(&self) -> Result<ConnectionResources, PermitError> {
        let mut endpoint = self.quota.lock()?;
        if endpoint.connections >= endpoint.max_connections {
            return Err(PermitError::Capacity);
        }
        let generation = next_generation(&mut endpoint)?;
        endpoint.connections += 1;
        drop(endpoint);
        let inner = Arc::new(ConnectionInner {
            endpoint: self.quota.clone(),
            state: Mutex::new(ConnectionState {
                generation,
                open: true,
                max_pending: self.connection_limits.max_pending,
                max_active: self.connection_limits.max_active,
                pending: 0,
                active: 0,
                operations: Default::default(),
                corrupted: false,
            }),
            _open_lease: OpenConnectionLease {
                endpoint: self.quota.clone(),
            },
        });
        Ok(ConnectionResources {
            inner: Some(inner),
            generation,
        })
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> QuotaSnapshot {
        let endpoint = self.quota.lock_for_drop();
        QuotaSnapshot {
            endpoint_pending: endpoint.pending,
            endpoint_active_subscriptions: endpoint.active_subscriptions,
            endpoint_connections: endpoint.connections,
            connection_pending: 0,
            connection_active: 0,
            corrupted: endpoint.corrupted,
        }
    }
}

/// Endpoint pending setup ownership before source initialization.
pub(crate) struct PendingEndpointOperation {
    endpoint: EndpointQuotaOwner,
    generation: OwnerGeneration,
    live: bool,
}

impl PendingEndpointOperation {
    pub(crate) const fn generation(&self) -> OwnerGeneration {
        self.generation
    }

    pub(crate) fn activate_subscription(
        mut self,
    ) -> Result<SharedActiveSubscriptionPermit, PermitError> {
        let mut endpoint = self.endpoint.lock()?;
        if endpoint.active_subscriptions >= endpoint.max_active_subscriptions {
            return Err(PermitError::Capacity);
        }
        if endpoint.pending == 0 {
            endpoint.corrupted = true;
            return Err(PermitError::Invariant);
        }
        endpoint.pending -= 1;
        endpoint.active_subscriptions += 1;
        self.live = false;
        Ok(SharedActiveSubscriptionPermit::new(
            ActiveSubscriptionPermit {
                endpoint: self.endpoint.clone(),
                live: true,
            },
            self.generation,
        ))
    }

    pub(crate) fn revoke(mut self) -> RevokedPendingEndpointCharge {
        self.live = false;
        RevokedPendingEndpointCharge {
            endpoint: self.endpoint.clone(),
            live: true,
        }
    }
}

impl Drop for PendingEndpointOperation {
    fn drop(&mut self) {
        if self.live {
            let mut endpoint = self.endpoint.lock_for_drop();
            release_endpoint_pending(&mut endpoint);
        }
    }
}

/// Physical pending-setup charge after logical cancellation.
pub(crate) struct RevokedPendingEndpointCharge {
    endpoint: EndpointQuotaOwner,
    live: bool,
}

impl Drop for RevokedPendingEndpointCharge {
    fn drop(&mut self) {
        if self.live {
            let mut endpoint = self.endpoint.lock_for_drop();
            release_endpoint_pending(&mut endpoint);
            self.live = false;
        }
    }
}

/// Physical active-subscription charge retained through cleanup.
pub(crate) struct ActiveSubscriptionPermit {
    endpoint: EndpointQuotaOwner,
    live: bool,
}

/// Shared ownership of one existing active-subscription charge.
#[derive(Clone)]
pub(crate) struct SharedActiveSubscriptionPermit {
    inner: Arc<SharedActiveSubscriptionPermitInner>,
}

struct SharedActiveSubscriptionPermitInner {
    generation: OwnerGeneration,
    _permit: ActiveSubscriptionPermit,
}

impl SharedActiveSubscriptionPermit {
    fn new(permit: ActiveSubscriptionPermit, generation: OwnerGeneration) -> Self {
        Self {
            inner: Arc::new(SharedActiveSubscriptionPermitInner {
                generation,
                _permit: permit,
            }),
        }
    }

    pub(crate) fn generation(&self) -> OwnerGeneration {
        self.inner.generation
    }
}

impl Drop for ActiveSubscriptionPermit {
    fn drop(&mut self) {
        if self.live {
            let mut endpoint = self.endpoint.lock_for_drop();
            release_endpoint_active(&mut endpoint);
            self.live = false;
        }
    }
}
