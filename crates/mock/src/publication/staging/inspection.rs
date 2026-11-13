//! Inspect actual staging charges and physical liveness in deterministic tests.

use std::sync::{Arc, Weak};

use super::{DispatchStaging, Inner, StagedBatch, Usage};
use crate::publication::PublicationError;

pub(in crate::publication) struct StagingLiveness {
    inner: Weak<Inner>,
    usage: Weak<Usage>,
}

impl DispatchStaging {
    pub(in crate::publication) fn take(&self) -> Result<Option<StagedBatch>, PublicationError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        Ok(state.queue.pop_front())
    }

    pub(in crate::publication) fn usage(&self) -> (usize, usize) {
        let usage = self.inner.usage.values.lock().unwrap();
        (usage.slots, usage.bytes)
    }

    pub(in crate::publication) fn liveness(&self) -> StagingLiveness {
        StagingLiveness {
            inner: Arc::downgrade(&self.inner),
            usage: Arc::downgrade(&self.inner.usage),
        }
    }
}

impl StagingLiveness {
    pub(in crate::publication) fn any_owner_alive(&self) -> bool {
        self.inner.upgrade().is_some() || self.usage.upgrade().is_some()
    }
}
