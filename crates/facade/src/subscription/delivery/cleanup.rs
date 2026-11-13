//! Logical revocation and bounded generation-keyed physical cleanup ownership.

#[cfg(feature = "subscription-worker")]
mod handoff;
mod polling;

use std::any::Any;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::OwnerGeneration;
use super::{
    RevokedConnectionCharge, RevokedConnectionOwner, RevokedPendingConnectionCharge,
    RevokedPendingEndpointCharge, SharedActiveSubscriptionPermit,
};
use polling::OwnedQuarantinedWork;

#[cfg(feature = "subscription-worker")]
use crate::subscription::worker::{EndpointWorkerOwner, WorkerGenerationSlot};

/// Cooperative cancellation shared with one physical source owner.
#[derive(Clone, Debug, Default)]
pub(crate) struct CancellationFlag {
    cancelled: Arc<AtomicBool>,
}

impl CancellationFlag {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Exact-generation authority checked immediately before every emission.
#[derive(Clone, Debug)]
pub(crate) struct EmissionAuthority {
    generation: OwnerGeneration,
    revoked: Arc<AtomicBool>,
}

impl EmissionAuthority {
    pub(crate) fn new(generation: OwnerGeneration) -> Self {
        Self {
            generation,
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    pub(crate) fn can_emit(&self, generation: OwnerGeneration) -> bool {
        generation == self.generation && !self.revoked.load(Ordering::Acquire)
    }
}

/// Globally unique generation key after every logical ID has been released.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct QuarantineKey(OwnerGeneration);

impl QuarantineKey {
    pub(crate) const fn new(generation: OwnerGeneration) -> Self {
        Self(generation)
    }
}

/// Endpoint-owned registry polled by one external maintenance driver.
#[derive(Clone)]
pub(crate) struct QuarantineRegistry {
    inner: Arc<QuarantineShared>,
}

struct QuarantineShared {
    state: Mutex<QuarantineState>,
    #[cfg(feature = "subscription-worker")]
    worker: Mutex<Option<EndpointWorkerOwner>>,
}

struct QuarantineState {
    max_entries: usize,
    slots: BTreeMap<QuarantineKey, CleanupSlot>,
    cursor: Option<QuarantineKey>,
    corrupted: bool,
}

enum CleanupSlot {
    Reserved,
    Quarantined(OwnedQuarantinedWork),
    Polling,
    Poisoned { _work: OwnedQuarantinedWork },
}

/// Move-only proof that cleanup capacity was reserved during admission.
pub(crate) struct CleanupTicket {
    registry: QuarantineRegistry,
    key: QuarantineKey,
    #[cfg(feature = "subscription-worker")]
    worker_slot: Option<WorkerGenerationSlot>,
    live: bool,
}

pub(crate) enum CleanupPhysicalCharge {
    PendingHttp {
        _permit: RevokedPendingEndpointCharge,
    },
    PendingConnection {
        _permit: RevokedPendingConnectionCharge,
    },
    Http {
        _permit: SharedActiveSubscriptionPermit,
    },
    Connection {
        _permit: RevokedConnectionCharge,
    },
    ConnectionOwner {
        _permit: RevokedConnectionOwner,
    },
}

impl CleanupTicket {
    /// Moves erased cleanup owners into the already allocated registry slot.
    pub(crate) fn quarantine_erased(
        mut self,
        future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
        charges: Box<dyn Any + Send + 'static>,
    ) {
        self.commit(OwnedQuarantinedWork::new(future, charges));
    }

    pub(crate) fn quarantine_completed(mut self, charges: CleanupPhysicalCharge) {
        let work = OwnedQuarantinedWork::completed(charges);
        #[cfg(feature = "subscription-worker")]
        let work = work.with_completed_slot(self.worker_slot.take());
        self.commit(work);
        #[cfg(feature = "subscription-worker")]
        self.registry.handoff_closed_workers();
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn spawn_worker<F>(
        &self,
        future: F,
    ) -> Result<
        crate::subscription::worker::ConcreteWorker<F::Output>,
        crate::subscription::worker::SubscriptionWorkerRuntimeError,
    >
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let slot = self
            .worker_slot
            .as_ref()
            .ok_or(crate::subscription::worker::SubscriptionWorkerRuntimeError::Invariant)?;
        crate::subscription::worker::spawn_worker(slot, future)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn quarantine_worker(
        mut self,
        completion: crate::subscription::worker::ErasedWorkerCompletion,
        physical: CleanupPhysicalCharge,
        retained: crate::subscription::worker::WorkerRetainedState,
    ) {
        let Some(slot) = self.worker_slot.take() else {
            std::mem::forget(completion);
            std::mem::forget(physical);
            std::mem::forget(retained);
            self.live = false;
            return;
        };
        self.commit(OwnedQuarantinedWork::worker(
            completion, physical, retained, slot,
        ));
        // A late completion may arrive after guard detach already scanned the
        // registry. Exact registration closure makes this race monotonic.
        self.registry.handoff_closed_workers();
    }

    fn commit(&mut self, work: OwnedQuarantinedWork) {
        let (mut state, lock_corrupted) = self.registry.lock_recover();
        let Some(slot) = state.slots.get_mut(&self.key) else {
            state.corrupted = true;
            drop(state);
            std::mem::forget(work);
            self.live = false;
            return;
        };
        if !matches!(slot, CleanupSlot::Reserved) {
            state.corrupted = true;
            drop(state);
            std::mem::forget(work);
            self.live = false;
            return;
        }
        *slot = CleanupSlot::Quarantined(work);
        state.corrupted |= lock_corrupted;
        self.live = false;
    }
}

impl Drop for CleanupTicket {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        let (mut state, lock_corrupted) = self.registry.lock_recover();
        if matches!(state.slots.get(&self.key), Some(CleanupSlot::Reserved)) {
            state.slots.remove(&self.key);
        } else {
            state.corrupted = true;
        }
        state.corrupted |= lock_corrupted;
        self.live = false;
    }
}

impl QuarantineRegistry {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            inner: Arc::new(QuarantineShared {
                state: Mutex::new(QuarantineState {
                    max_entries,
                    slots: BTreeMap::new(),
                    cursor: None,
                    corrupted: false,
                }),
                #[cfg(feature = "subscription-worker")]
                worker: Mutex::new(None),
            }),
        }
    }

    /// Reserves one exact generation before an admitted owner escapes.
    pub(crate) fn reserve(&self, key: QuarantineKey) -> Result<CleanupTicket, QuarantineError> {
        #[cfg(feature = "subscription-worker")]
        let worker_slot = {
            let worker = self
                .inner
                .worker
                .lock()
                .map_err(|_| QuarantineError::Invariant)?;
            worker
                .as_ref()
                .map(EndpointWorkerOwner::reserve_generation)
                .transpose()
                .map_err(|_| QuarantineError::Invariant)?
        };
        let mut state = self.lock()?;
        if state.slots.len() >= state.max_entries {
            return Err(QuarantineError::Capacity);
        }
        if state.slots.contains_key(&key) {
            return Err(QuarantineError::DuplicateGeneration);
        }
        state.slots.insert(key.clone(), CleanupSlot::Reserved);
        Ok(CleanupTicket {
            registry: self.clone(),
            key,
            #[cfg(feature = "subscription-worker")]
            worker_slot,
            live: true,
        })
    }

    /// Retains a future and its physical charges without creating a task.
    #[cfg(test)]
    pub(crate) fn insert<F, C>(
        &self,
        key: QuarantineKey,
        future: F,
        charges: C,
    ) -> Result<(), QuarantineInsertFailure<F, C>>
    where
        F: Future<Output = ()> + Send + 'static,
        C: Any + Send + 'static,
    {
        let ticket = match self.reserve(key.clone()) {
            Ok(ticket) => ticket,
            Err(error) => {
                return Err(QuarantineInsertFailure {
                    error,
                    key,
                    future,
                    charges,
                });
            }
        };
        ticket.quarantine_erased(Box::pin(future), Box::new(charges));
        Ok(())
    }

    pub(crate) fn len(&self) -> Result<usize, QuarantineError> {
        let state = self.lock()?;
        Ok(state
            .slots
            .values()
            .filter(|slot| !matches!(slot, CleanupSlot::Reserved))
            .count())
    }

    #[cfg(test)]
    pub(crate) fn retained_slots(&self) -> usize {
        self.lock_recover().0.slots.len()
    }

    fn lock(&self) -> Result<MutexGuard<'_, QuarantineState>, QuarantineError> {
        match self.inner.state.lock() {
            Ok(guard) if !guard.corrupted => Ok(guard),
            Ok(_) => Err(QuarantineError::Invariant),
            Err(poisoned) => {
                poisoned.into_inner().corrupted = true;
                Err(QuarantineError::Invariant)
            }
        }
    }

    fn lock_recover(&self) -> (MutexGuard<'_, QuarantineState>, bool) {
        match self.inner.state.lock() {
            Ok(guard) => {
                let corrupted = guard.corrupted;
                (guard, corrupted)
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.corrupted = true;
                (guard, true)
            }
        }
    }
}

/// Failed insertion that returns every physical owner to the caller.
#[cfg(test)]
pub(crate) struct QuarantineInsertFailure<F, C> {
    error: QuarantineError,
    key: QuarantineKey,
    future: F,
    charges: C,
}

#[cfg(test)]
impl<F, C> QuarantineInsertFailure<F, C> {
    pub(crate) const fn error(&self) -> QuarantineError {
        self.error
    }

    pub(crate) fn into_parts(self) -> (QuarantineKey, F, C) {
        (self.key, self.future, self.charges)
    }
}

/// Quarantine insertion or ownership bookkeeping failed closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuarantineError {
    Capacity,
    DuplicateGeneration,
    Invariant,
}
