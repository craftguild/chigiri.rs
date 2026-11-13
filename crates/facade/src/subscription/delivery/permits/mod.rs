//! Atomic endpoint and connection admission with generation-safe RAII release.

mod connection;
mod connection_charge;
mod connection_owner;
mod endpoint;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) use connection::{
    ActiveConnectionOperation, PendingConnectionOperation, RevokedConnectionCharge,
    RevokedPendingConnectionCharge,
};
pub(crate) use connection_charge::SharedActiveConnectionPermit;
#[cfg(feature = "subscription-worker")]
pub(crate) use connection_owner::SharedConnectionPhysicalLease;
pub(crate) use connection_owner::{
    ConnectionAdmissionResources, ConnectionResources, RevokedConnectionOwner,
};
pub(crate) use endpoint::{
    DeliveryResources, PendingEndpointOperation, RevokedPendingEndpointCharge,
    SharedActiveSubscriptionPermit,
};

/// Operation kinds charged against a multiplexed connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationKind {
    Query,
    Mutation,
    Subscription,
}

/// Opaque, endpoint-issued owner generation that callers cannot reuse.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct OwnerGeneration(u64);

/// Admission or release could not preserve the quota invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PermitError {
    Capacity,
    DuplicateId,
    InvalidId,
    GenerationExhausted,
    StaleGeneration,
    Invariant,
}

/// Safe counter view for focused diagnostics and deterministic tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QuotaSnapshot {
    pub(crate) endpoint_pending: usize,
    pub(crate) endpoint_active_subscriptions: usize,
    pub(crate) endpoint_connections: usize,
    pub(crate) connection_pending: usize,
    pub(crate) connection_active: usize,
    pub(crate) corrupted: bool,
}

#[derive(Clone)]
struct EndpointQuotaOwner {
    inner: Arc<Mutex<EndpointQuotaState>>,
}

struct EndpointQuotaState {
    max_pending: usize,
    max_active_subscriptions: usize,
    max_connections: usize,
    pending: usize,
    active_subscriptions: usize,
    connections: usize,
    last_generation: u64,
    corrupted: bool,
}

impl EndpointQuotaOwner {
    fn lock(&self) -> Result<MutexGuard<'_, EndpointQuotaState>, PermitError> {
        match self.inner.lock() {
            Ok(guard) if !guard.corrupted => Ok(guard),
            Ok(_) => Err(PermitError::Invariant),
            Err(poisoned) => {
                poisoned.into_inner().corrupted = true;
                Err(PermitError::Invariant)
            }
        }
    }

    fn lock_for_drop(&self) -> MutexGuard<'_, EndpointQuotaState> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.corrupted = true;
                guard
            }
        }
    }
}

struct ConnectionInner {
    endpoint: EndpointQuotaOwner,
    state: Mutex<ConnectionState>,
    _open_lease: OpenConnectionLease,
}

struct ConnectionState {
    generation: OwnerGeneration,
    open: bool,
    max_pending: usize,
    max_active: usize,
    pending: usize,
    active: usize,
    operations: BTreeMap<Arc<str>, LogicalOperation>,
    corrupted: bool,
}

impl ConnectionInner {
    fn lock(&self) -> Result<MutexGuard<'_, ConnectionState>, PermitError> {
        match self.state.lock() {
            Ok(guard) if !guard.corrupted => Ok(guard),
            Ok(_) => Err(PermitError::Invariant),
            Err(poisoned) => {
                poisoned.into_inner().corrupted = true;
                Err(PermitError::Invariant)
            }
        }
    }

    fn lock_for_drop(&self) -> MutexGuard<'_, ConnectionState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.corrupted = true;
                guard
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LogicalOperation {
    generation: OwnerGeneration,
    phase: OperationPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationPhase {
    Pending,
    Active(OperationKind),
}

struct OpenConnectionLease {
    endpoint: EndpointQuotaOwner,
}

impl Drop for OpenConnectionLease {
    fn drop(&mut self) {
        let mut endpoint = self.endpoint.lock_for_drop();
        release_endpoint_connection(&mut endpoint);
    }
}

fn next_generation(state: &mut EndpointQuotaState) -> Result<OwnerGeneration, PermitError> {
    let next = state
        .last_generation
        .checked_add(1)
        .ok_or(PermitError::GenerationExhausted)?;
    state.last_generation = next;
    Ok(OwnerGeneration(next))
}

fn release_endpoint_pending(state: &mut EndpointQuotaState) {
    if !decrement(&mut state.pending) {
        state.corrupted = true;
    }
}

fn release_endpoint_active(state: &mut EndpointQuotaState) {
    if !decrement(&mut state.active_subscriptions) {
        state.corrupted = true;
    }
}

fn release_endpoint_connection(state: &mut EndpointQuotaState) {
    if !decrement(&mut state.connections) {
        state.corrupted = true;
    }
}

fn release_connection_pending(state: &mut ConnectionState) {
    if !decrement(&mut state.pending) {
        state.corrupted = true;
    }
}

fn release_connection_active(state: &mut ConnectionState) {
    if !decrement(&mut state.active) {
        state.corrupted = true;
    }
}

fn decrement(value: &mut usize) -> bool {
    let Some(next) = value.checked_sub(1) else {
        return false;
    };
    *value = next;
    true
}

fn exact_operation(
    operation: Option<&LogicalOperation>,
    generation: OwnerGeneration,
    phase: OperationPhase,
) -> bool {
    operation.is_some_and(|entry| entry.generation == generation && entry.phase == phase)
}
