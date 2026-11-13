//! Move-only connection ownership and weak operation-admission views.

use std::sync::{Arc, Weak};

#[cfg(test)]
use super::QuotaSnapshot;
use super::connection::PendingConnectionOperation;
use super::{
    ConnectionInner, LogicalOperation, OperationPhase, OwnerGeneration, PermitError,
    next_generation,
};

pub(crate) struct ConnectionResources {
    pub(super) inner: Option<Arc<ConnectionInner>>,
    pub(super) generation: OwnerGeneration,
}

impl ConnectionResources {
    pub(crate) const fn generation(&self) -> OwnerGeneration {
        self.generation
    }

    pub(crate) fn admission(&self) -> ConnectionAdmissionResources {
        let inner = self.inner.as_ref().expect("live connection owner");
        ConnectionAdmissionResources {
            inner: Arc::downgrade(inner),
            generation: self.generation,
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn physical_lease(&self) -> Result<SharedConnectionPhysicalLease, PermitError> {
        let inner = self.inner.as_ref().ok_or(PermitError::Invariant)?;
        let connection = inner.lock()?;
        if connection.generation != self.generation || !connection.open {
            return Err(PermitError::StaleGeneration);
        }
        drop(connection);
        Ok(SharedConnectionPhysicalLease {
            _inner: Arc::clone(inner),
            generation: self.generation,
        })
    }

    #[cfg(test)]
    pub(crate) fn begin(&self, id: Arc<str>) -> Result<PendingConnectionOperation, PermitError> {
        self.admission().begin(id)
    }

    pub(crate) fn revoke(mut self) -> RevokedConnectionOwner {
        let inner = self.inner.take().expect("live connection owner");
        {
            let _endpoint = inner.endpoint.lock_for_drop();
            let mut connection = inner.lock_for_drop();
            if connection.generation != self.generation || !connection.open {
                connection.corrupted = true;
            }
            connection.open = false;
        }
        RevokedConnectionOwner { _inner: inner }
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> QuotaSnapshot {
        snapshot(self.inner.as_ref().expect("live connection owner"))
    }
}

#[cfg(feature = "subscription-worker")]
#[derive(Clone)]
pub(crate) struct SharedConnectionPhysicalLease {
    _inner: Arc<ConnectionInner>,
    generation: OwnerGeneration,
}

#[cfg(feature = "subscription-worker")]
impl SharedConnectionPhysicalLease {
    pub(crate) const fn generation(&self) -> OwnerGeneration {
        self.generation
    }
}

impl Drop for ConnectionResources {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        {
            let _endpoint = inner.endpoint.lock_for_drop();
            let mut connection = inner.lock_for_drop();
            if connection.generation == self.generation {
                connection.open = false;
            } else {
                connection.corrupted = true;
            }
        }
        drop(inner);
    }
}

#[derive(Clone)]
pub(crate) struct ConnectionAdmissionResources {
    inner: Weak<ConnectionInner>,
    generation: OwnerGeneration,
}

impl ConnectionAdmissionResources {
    pub(crate) fn begin(&self, id: Arc<str>) -> Result<PendingConnectionOperation, PermitError> {
        if id.is_empty() || id.len() > 256 {
            return Err(PermitError::InvalidId);
        }
        let inner = self.inner.upgrade().ok_or(PermitError::StaleGeneration)?;
        let mut endpoint = inner.endpoint.lock()?;
        let mut connection = inner.lock()?;
        if connection.generation != self.generation || !connection.open {
            return Err(PermitError::StaleGeneration);
        }
        if connection.operations.contains_key(id.as_ref()) {
            return Err(PermitError::DuplicateId);
        }
        if endpoint.pending >= endpoint.max_pending || connection.pending >= connection.max_pending
        {
            return Err(PermitError::Capacity);
        }
        let generation = next_generation(&mut endpoint)?;
        connection.operations.insert(
            Arc::clone(&id),
            LogicalOperation {
                generation,
                phase: OperationPhase::Pending,
            },
        );
        endpoint.pending += 1;
        connection.pending += 1;
        drop(connection);
        drop(endpoint);
        Ok(PendingConnectionOperation::new(inner, id, generation))
    }
}

pub(crate) struct RevokedConnectionOwner {
    _inner: Arc<ConnectionInner>,
}

#[cfg(test)]
fn snapshot(inner: &ConnectionInner) -> QuotaSnapshot {
    let endpoint = inner.endpoint.lock_for_drop();
    let connection = inner.lock_for_drop();
    QuotaSnapshot {
        endpoint_pending: endpoint.pending,
        endpoint_active_subscriptions: endpoint.active_subscriptions,
        endpoint_connections: endpoint.connections,
        connection_pending: connection.pending,
        connection_active: connection.active,
        corrupted: endpoint.corrupted || connection.corrupted,
    }
}
