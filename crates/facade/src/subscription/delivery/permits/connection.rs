//! Multiplexed operation generations and atomic pending-to-active transfer.

use std::sync::Arc;

use super::{
    ConnectionInner, LogicalOperation, OperationKind, OperationPhase, OwnerGeneration, PermitError,
    SharedActiveConnectionPermit, exact_operation, release_connection_pending,
    release_endpoint_pending,
};

/// A pending setup charged to both endpoint and connection quotas.
pub(crate) struct PendingConnectionOperation {
    connection: Arc<ConnectionInner>,
    id: Arc<str>,
    generation: OwnerGeneration,
    live: bool,
}

impl PendingConnectionOperation {
    pub(super) fn new(
        connection: Arc<ConnectionInner>,
        id: Arc<str>,
        generation: OwnerGeneration,
    ) -> Self {
        Self {
            connection,
            id,
            generation,
            live: true,
        }
    }

    pub(crate) const fn generation(&self) -> OwnerGeneration {
        self.generation
    }

    pub(crate) fn activate(
        mut self,
        kind: OperationKind,
    ) -> Result<ActiveConnectionOperation, PermitError> {
        let mut endpoint = self.connection.endpoint.lock()?;
        let mut connection = self.connection.lock()?;
        if !exact_operation(
            connection.operations.get(self.id.as_ref()),
            self.generation,
            OperationPhase::Pending,
        ) {
            return Err(PermitError::StaleGeneration);
        }
        if connection.active >= connection.max_active
            || (kind == OperationKind::Subscription
                && endpoint.active_subscriptions >= endpoint.max_active_subscriptions)
        {
            return Err(PermitError::Capacity);
        }
        if endpoint.pending == 0 || connection.pending == 0 {
            endpoint.corrupted = true;
            connection.corrupted = true;
            return Err(PermitError::Invariant);
        }
        connection.operations.insert(
            Arc::clone(&self.id),
            LogicalOperation {
                generation: self.generation,
                phase: OperationPhase::Active(kind),
            },
        );
        endpoint.pending -= 1;
        connection.pending -= 1;
        connection.active += 1;
        if kind == OperationKind::Subscription {
            endpoint.active_subscriptions += 1;
        }
        self.live = false;
        Ok(ActiveConnectionOperation {
            connection: Arc::clone(&self.connection),
            id: Arc::clone(&self.id),
            generation: self.generation,
            kind,
            physical: SharedActiveConnectionPermit::new(
                Arc::clone(&self.connection),
                self.generation,
                kind,
            ),
            logical: true,
            live: true,
        })
    }

    pub(crate) fn revoke(mut self) -> RevokedPendingConnectionCharge {
        {
            let _endpoint = self.connection.endpoint.lock_for_drop();
            let mut connection = self.connection.lock_for_drop();
            if exact_operation(
                connection.operations.get(self.id.as_ref()),
                self.generation,
                OperationPhase::Pending,
            ) {
                connection.operations.remove(self.id.as_ref());
            } else {
                connection.corrupted = true;
            }
        }
        self.live = false;
        RevokedPendingConnectionCharge {
            connection: Arc::clone(&self.connection),
            live: true,
        }
    }
}

impl Drop for PendingConnectionOperation {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        let mut endpoint = self.connection.endpoint.lock_for_drop();
        let mut connection = self.connection.lock_for_drop();
        if exact_operation(
            connection.operations.get(self.id.as_ref()),
            self.generation,
            OperationPhase::Pending,
        ) {
            connection.operations.remove(self.id.as_ref());
        } else {
            connection.corrupted = true;
        }
        release_endpoint_pending(&mut endpoint);
        release_connection_pending(&mut connection);
    }
}

/// Physical pending quotas after the wire ID has been released.
pub(crate) struct RevokedPendingConnectionCharge {
    connection: Arc<ConnectionInner>,
    live: bool,
}

impl Drop for RevokedPendingConnectionCharge {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        let mut endpoint = self.connection.endpoint.lock_for_drop();
        let mut connection = self.connection.lock_for_drop();
        release_endpoint_pending(&mut endpoint);
        release_connection_pending(&mut connection);
        self.live = false;
    }
}

/// Active logical generation plus its physical connection/subscription charge.
pub(crate) struct ActiveConnectionOperation {
    connection: Arc<ConnectionInner>,
    id: Arc<str>,
    generation: OwnerGeneration,
    kind: OperationKind,
    physical: SharedActiveConnectionPermit,
    logical: bool,
    live: bool,
}

impl ActiveConnectionOperation {
    #[cfg(test)]
    pub(crate) const fn generation(&self) -> OwnerGeneration {
        self.generation
    }

    pub(crate) fn physical_lease(&self) -> Result<SharedActiveConnectionPermit, PermitError> {
        if !self.live || !self.logical || self.physical.generation() != self.generation {
            return Err(PermitError::StaleGeneration);
        }
        let connection = self.connection.lock()?;
        if !connection.open
            || !exact_operation(
                connection.operations.get(self.id.as_ref()),
                self.generation,
                OperationPhase::Active(self.kind),
            )
        {
            return Err(PermitError::StaleGeneration);
        }
        Ok(self.physical.clone())
    }

    pub(crate) fn revoke(mut self) -> RevokedConnectionCharge {
        {
            let _endpoint = self.connection.endpoint.lock_for_drop();
            let mut connection = self.connection.lock_for_drop();
            if exact_operation(
                connection.operations.get(self.id.as_ref()),
                self.generation,
                OperationPhase::Active(self.kind),
            ) {
                connection.operations.remove(self.id.as_ref());
            } else {
                connection.corrupted = true;
            }
        }
        self.logical = false;
        self.live = false;
        RevokedConnectionCharge {
            _physical: self.physical.clone(),
        }
    }
}

impl Drop for ActiveConnectionOperation {
    fn drop(&mut self) {
        if self.live {
            release_logical(&self.connection, &self.id, self.generation, self.kind);
            self.live = false;
        }
    }
}

/// Physical old-generation charge after immediate logical revocation.
pub(crate) struct RevokedConnectionCharge {
    _physical: SharedActiveConnectionPermit,
}

fn release_logical(
    owner: &ConnectionInner,
    id: &str,
    generation: OwnerGeneration,
    kind: OperationKind,
) {
    let _endpoint = owner.endpoint.lock_for_drop();
    let mut connection = owner.lock_for_drop();
    if exact_operation(
        connection.operations.get(id),
        generation,
        OperationPhase::Active(kind),
    ) {
        connection.operations.remove(id);
    } else {
        connection.corrupted = true;
    }
}
