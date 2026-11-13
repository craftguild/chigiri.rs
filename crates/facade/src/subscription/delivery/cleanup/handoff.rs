//! Transfer concrete joins and release completed SDK metadata after close.
//!
//! Endpoint clones may remain alive for outcome inspection. Recovery must not
//! depend on dropping that endpoint graph or on an application reaper poll.

use super::{CleanupSlot, QuarantineError, QuarantineRegistry};

impl QuarantineRegistry {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn register_worker_runtime(
        &self,
    ) -> Result<crate::subscription::worker::SubscriptionRuntimeGuard, QuarantineError> {
        let mut worker = self
            .inner
            .worker
            .lock()
            .map_err(|_| QuarantineError::Invariant)?;
        if worker.is_some() {
            return Err(QuarantineError::Invariant);
        }
        let owner = crate::subscription::worker::register_endpoint()
            .map_err(|_| QuarantineError::Invariant)?;
        *worker = Some(owner);
        Ok(crate::subscription::worker::SubscriptionRuntimeGuard::new(
            self.clone(),
        ))
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn detach_worker_runtime(
        &self,
    ) -> Result<crate::subscription::worker::EndpointWorkerShutdown, QuarantineError> {
        let owner = self
            .inner
            .worker
            .lock()
            .map_err(|_| QuarantineError::Invariant)?
            .take()
            .ok_or(QuarantineError::Invariant)?;
        let shutdown = owner.shutdown();
        self.handoff_closed_workers();
        Ok(shutdown)
    }
    pub(super) fn handoff_closed_workers(&self) {
        loop {
            let work = {
                let (mut state, _) = self.lock_recover();
                let key = state.slots.iter().find_map(|(key, slot)| match slot {
                    CleanupSlot::Quarantined(work) | CleanupSlot::Poisoned { _work: work }
                        if work.needs_worker_handoff() =>
                    {
                        Some(key.clone())
                    }
                    _ => None,
                });
                key.and_then(|key| state.slots.remove(&key))
            };
            let Some(work) = work else {
                return;
            };
            // A worker transfers its original join and charge to its process
            // slot. A completed entry has only closed SDK physical ownership.
            // No application future is polled or dropped under either lock.
            drop(work);
        }
    }
}
