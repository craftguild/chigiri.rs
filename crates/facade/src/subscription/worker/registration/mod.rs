//! Exact endpoint cleanup generations registered with the process reaper.

mod generation;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Context, Poll, Waker};

use super::completion::RetiredWorker;
use super::error::SubscriptionWorkerRuntimeError;
use super::process::{ProcessShared, notify_process, process_accepting, unregister_if_drained};
use generation::GenerationSlot;
pub(crate) use generation::WorkerGenerationSlot;
pub(crate) use generation::WorkerSlotUse;

pub(super) struct WorkerRegistration {
    id: u64,
    pub(super) process: Weak<ProcessShared>,
    closing: AtomicBool,
    state: Mutex<RegistrationState>,
}

struct RegistrationState {
    next_slot: u64,
    slots: BTreeMap<u64, Arc<GenerationSlot>>,
    cursor: Option<u64>,
    drain_waker: Option<Waker>,
    corrupted: bool,
}

impl WorkerRegistration {
    #[cfg(test)]
    pub(super) fn diagnostic_for_test(&self) -> String {
        let slots = match self.state.try_lock() {
            Ok(state) => state.slots.values().cloned().collect::<Vec<_>>(),
            Err(error) => return format!("{error}"),
        };
        format!(
            "{:?}",
            slots
                .iter()
                .map(|slot| slot.diagnostic_for_test())
                .collect::<Vec<_>>()
        )
    }

    #[cfg(test)]
    pub(super) fn retained_states_for_test(&self) -> [usize; 7] {
        let mut counts = [0; 7];
        for slot in self.lock_recover().slots.values() {
            counts[slot.state_index_for_test()] += 1;
        }
        counts
    }

    pub(super) fn new(id: u64, process: Weak<ProcessShared>) -> Arc<Self> {
        Arc::new(Self {
            id,
            process,
            closing: AtomicBool::new(false),
            state: Mutex::new(RegistrationState {
                next_slot: 0,
                slots: BTreeMap::new(),
                cursor: None,
                drain_waker: None,
                corrupted: false,
            }),
        })
    }

    pub(super) const fn id(&self) -> u64 {
        self.id
    }

    /// Creates the process record for the exact cleanup generation reservation.
    pub(super) fn reserve_generation(
        self: &Arc<Self>,
    ) -> Result<WorkerGenerationSlot, SubscriptionWorkerRuntimeError> {
        if self.closing.load(Ordering::Acquire) {
            return Err(SubscriptionWorkerRuntimeError::EndpointClosed);
        }
        let process = self
            .process
            .upgrade()
            .ok_or(SubscriptionWorkerRuntimeError::Shutdown)?;
        if !process_accepting(&process) {
            return Err(SubscriptionWorkerRuntimeError::Shutdown);
        }
        let mut state = self.lock()?;
        let id = state
            .next_slot
            .checked_add(1)
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
        state.next_slot = id;
        let slot = GenerationSlot::new(Arc::downgrade(self), id);
        state.slots.insert(id, Arc::clone(&slot));
        Ok(WorkerGenerationSlot::new(slot))
    }

    pub(super) fn close(&self) {
        self.closing.store(true, Ordering::Release);
        self.wake_drain();
        notify_process(&self.process);
    }

    pub(super) fn take_retired(&self) -> Option<(Arc<GenerationSlot>, RetiredWorker)> {
        let mut state = self.lock_recover();
        let next = state.cursor.and_then(|cursor| {
            state
                .slots
                .range((
                    std::ops::Bound::Excluded(cursor),
                    std::ops::Bound::Unbounded,
                ))
                .find_map(|(id, slot)| {
                    slot.take_retired()
                        .map(|record| (*id, Arc::clone(slot), record))
                })
        });
        let selected = next.or_else(|| {
            state.slots.iter().find_map(|(id, slot)| {
                slot.take_retired()
                    .map(|record| (*id, Arc::clone(slot), record))
            })
        });
        selected.map(|(id, slot, record)| {
            state.cursor = Some(id);
            (slot, record)
        })
    }

    pub(super) fn poll_drained(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        let mut replacement = Some(context.waker().clone());
        let mut state = self.lock_recover();
        let (result, previous) =
            if state.corrupted || state.slots.values().any(|slot| slot.poisoned()) {
                (
                    Poll::Ready(Err(SubscriptionWorkerRuntimeError::Poisoned)),
                    None,
                )
            } else if state.slots.is_empty() {
                (Poll::Ready(Ok(())), None)
            } else {
                let previous = state
                    .drain_waker
                    .replace(replacement.take().expect("proposed registration waker"));
                (Poll::Pending, previous)
            };
        drop(state);
        drop(previous);
        drop(replacement);
        result
    }

    pub(super) fn retired_count(&self) -> usize {
        self.lock_recover()
            .slots
            .values()
            .filter(|slot| slot.has_retired_worker())
            .count()
    }

    pub(super) fn drained(&self) -> bool {
        let state = self.lock_recover();
        self.closing.load(Ordering::Acquire) && !state.corrupted && state.slots.is_empty()
    }

    pub(super) fn remove_released(&self, id: u64) {
        let mut state = self.lock_recover();
        let removable = state.slots.get(&id).is_some_and(|slot| slot.released());
        if removable {
            state.slots.remove(&id);
        } else {
            state.corrupted = true;
        }
        drop(state);
        self.wake_drain();
        unregister_if_drained(self);
    }

    fn wake_drain(&self) {
        let waker = self.lock_recover().drain_waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    #[cfg(test)]
    fn lock_probe_for_test(self: &Arc<Self>) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        let registration = Arc::downgrade(self);
        Arc::new(move || {
            registration.upgrade().is_some_and(|registration| {
                matches!(
                    registration.state.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                )
            })
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, RegistrationState>, SubscriptionWorkerRuntimeError> {
        self.state
            .lock()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)
    }

    fn lock_recover(&self) -> MutexGuard<'_, RegistrationState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.corrupted = true;
                state
            }
        }
    }
}

pub(crate) struct EndpointWorkerOwner {
    registration: Option<Arc<WorkerRegistration>>,
}

impl EndpointWorkerOwner {
    pub(super) fn new(registration: Arc<WorkerRegistration>) -> Self {
        Self {
            registration: Some(registration),
        }
    }

    pub(crate) fn reserve_generation(
        &self,
    ) -> Result<WorkerGenerationSlot, SubscriptionWorkerRuntimeError> {
        self.registration
            .as_ref()
            .ok_or(SubscriptionWorkerRuntimeError::EndpointClosed)?
            .reserve_generation()
    }

    pub(crate) fn shutdown(mut self) -> EndpointWorkerShutdown {
        let registration = self
            .registration
            .take()
            .expect("live endpoint worker owner");
        registration.close();
        EndpointWorkerShutdown { registration }
    }
}

impl Drop for EndpointWorkerOwner {
    fn drop(&mut self) {
        if let Some(registration) = &self.registration {
            registration.close();
        }
    }
}

pub(crate) struct EndpointWorkerShutdown {
    registration: Arc<WorkerRegistration>,
}

#[cfg(test)]
impl EndpointWorkerShutdown {
    pub(crate) fn retained_states_for_test(&self) -> [usize; 7] {
        self.registration.retained_states_for_test()
    }
}

impl std::future::Future for EndpointWorkerShutdown {
    type Output = Result<(), SubscriptionWorkerRuntimeError>;

    fn poll(self: std::pin::Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.registration.poll_drained(context)
    }
}
