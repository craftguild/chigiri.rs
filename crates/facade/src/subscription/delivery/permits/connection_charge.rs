//! One active operation charge shared by cleanup and immutable socket frames.

use std::sync::Arc;

use super::{
    ConnectionInner, OperationKind, OwnerGeneration, release_connection_active,
    release_endpoint_active,
};

#[derive(Clone)]
pub(crate) struct SharedActiveConnectionPermit {
    inner: Arc<ActiveConnectionCharge>,
}

struct ActiveConnectionCharge {
    connection: Arc<ConnectionInner>,
    generation: OwnerGeneration,
    kind: OperationKind,
}

impl SharedActiveConnectionPermit {
    pub(super) fn new(
        connection: Arc<ConnectionInner>,
        generation: OwnerGeneration,
        kind: OperationKind,
    ) -> Self {
        Self {
            inner: Arc::new(ActiveConnectionCharge {
                connection,
                generation,
                kind,
            }),
        }
    }

    pub(crate) fn generation(&self) -> OwnerGeneration {
        self.inner.generation
    }
}

impl Drop for ActiveConnectionCharge {
    fn drop(&mut self) {
        let mut endpoint = self.connection.endpoint.lock_for_drop();
        let mut connection = self.connection.lock_for_drop();
        release_connection_active(&mut connection);
        if self.kind == OperationKind::Subscription {
            release_endpoint_active(&mut endpoint);
        }
    }
}
