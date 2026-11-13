//! One reusable concrete-worker slot for an exact cleanup generation.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use super::WorkerRegistration;
use crate::subscription::worker::completion::{RetiredWorker, WorkerCompletionOutcome};
use crate::subscription::worker::error::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::process::{mark_process_poisoned, notify_process};

pub(crate) struct GenerationSlot {
    registration: Weak<WorkerRegistration>,
    id: u64,
    state: Mutex<GenerationState>,
}

enum GenerationState {
    Idle { next_worker: u64 },
    Running { worker: u64 },
    Retired(RetiredWorker),
    Polling,
    Poisoned { _record: RetiredWorker },
    Failed,
    Released,
}

impl GenerationSlot {
    #[cfg(test)]
    pub(super) fn diagnostic_for_test(&self) -> String {
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(error) => return format!("slot {}: {error}", self.id),
        };
        let detail = match &*state {
            GenerationState::Idle { .. } => "Idle".to_owned(),
            GenerationState::Running { .. } => "Running".to_owned(),
            GenerationState::Retired(record) => {
                format!("Retired {}", record.native_state_for_test())
            }
            GenerationState::Polling => "Polling".to_owned(),
            GenerationState::Poisoned { _record } => {
                format!("Poisoned {}", _record.native_state_for_test())
            }
            GenerationState::Failed => "Failed".to_owned(),
            GenerationState::Released => "Released".to_owned(),
        };
        format!("slot {}: {detail}", self.id)
    }

    #[cfg(test)]
    pub(super) fn state_index_for_test(&self) -> usize {
        match &*self.lock_recover() {
            GenerationState::Idle { .. } => 0,
            GenerationState::Running { .. } => 1,
            GenerationState::Retired(_) => 2,
            GenerationState::Polling => 3,
            GenerationState::Poisoned { .. } => 4,
            GenerationState::Failed => 5,
            GenerationState::Released => 6,
        }
    }

    pub(super) fn new(registration: Weak<WorkerRegistration>, id: u64) -> Arc<Self> {
        Arc::new(Self {
            registration,
            id,
            state: Mutex::new(GenerationState::Idle { next_worker: 0 }),
        })
    }

    fn start(self: &Arc<Self>) -> Result<WorkerSlotUse, SubscriptionWorkerRuntimeError> {
        let mut state = self.lock()?;
        let GenerationState::Idle { next_worker } = *state else {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        };
        let worker = next_worker
            .checked_add(1)
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
        *state = GenerationState::Running { worker };
        Ok(WorkerSlotUse {
            slot: Arc::clone(self),
            worker,
            live: true,
        })
    }

    fn complete(&self, worker: u64) -> Result<(), SubscriptionWorkerRuntimeError> {
        let mut state = self.lock()?;
        if !matches!(*state, GenerationState::Running { worker: current } if current == worker) {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        *state = GenerationState::Idle {
            next_worker: worker,
        };
        Ok(())
    }

    fn retire(&self, worker: u64, record: RetiredWorker) {
        let mut state = self.lock_recover();
        if !matches!(*state, GenerationState::Running { worker: current } if current == worker) {
            drop(state);
            std::mem::forget(record);
            return;
        }
        *state = GenerationState::Retired(record);
        drop(state);
        if let Some(registration) = self.registration.upgrade() {
            notify_process(&registration.process);
        }
    }

    pub(super) fn take_retired(&self) -> Option<RetiredWorker> {
        let mut state = self.lock_recover();
        let GenerationState::Retired(_) = *state else {
            return None;
        };
        let GenerationState::Retired(record) =
            std::mem::replace(&mut *state, GenerationState::Polling)
        else {
            unreachable!("matched retired generation")
        };
        Some(record)
    }

    pub(super) fn has_retired_worker(&self) -> bool {
        matches!(&*self.lock_recover(), GenerationState::Retired(_))
    }

    pub(crate) fn restore_pending(&self, record: RetiredWorker) {
        let mut state = self.lock_recover();
        if !matches!(*state, GenerationState::Polling) {
            drop(state);
            std::mem::forget(record);
            return;
        }
        *state = GenerationState::Retired(record);
    }

    pub(crate) fn finish_polled(&self, record: RetiredWorker, outcome: WorkerCompletionOutcome) {
        let mut state = self.lock_recover();
        if !matches!(*state, GenerationState::Polling) {
            drop(state);
            std::mem::forget(record);
            return;
        }
        match outcome {
            WorkerCompletionOutcome::Completed => {
                *state = GenerationState::Released;
                drop(state);
                if record.finish() {
                    if let Some(registration) = self.registration.upgrade() {
                        registration.remove_released(self.id);
                    }
                } else {
                    *self.lock_recover() = GenerationState::Failed;
                    if let Some(registration) = self.registration.upgrade() {
                        mark_process_poisoned(&registration.process);
                    }
                }
                return;
            }
            WorkerCompletionOutcome::Poisoned => {
                *state = GenerationState::Poisoned { _record: record };
            }
        }
        drop(state);
        let Some(registration) = self.registration.upgrade() else {
            return;
        };
        if outcome == WorkerCompletionOutcome::Poisoned {
            mark_process_poisoned(&registration.process);
        } else {
            registration.remove_released(self.id);
        }
    }

    fn release(&self) -> Result<(), SubscriptionWorkerRuntimeError> {
        let mut state = self.lock()?;
        if !matches!(*state, GenerationState::Idle { .. }) {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        *state = GenerationState::Released;
        drop(state);
        if let Some(registration) = self.registration.upgrade() {
            registration.remove_released(self.id);
        }
        Ok(())
    }

    pub(super) fn released(&self) -> bool {
        matches!(*self.lock_recover(), GenerationState::Released)
    }

    pub(super) fn poisoned(&self) -> bool {
        matches!(
            *self.lock_recover(),
            GenerationState::Poisoned { .. } | GenerationState::Failed
        )
    }

    fn lock(&self) -> Result<MutexGuard<'_, GenerationState>, SubscriptionWorkerRuntimeError> {
        self.state
            .lock()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)
    }

    fn lock_recover(&self) -> MutexGuard<'_, GenerationState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

pub(crate) struct WorkerGenerationSlot {
    slot: Arc<GenerationSlot>,
    released: bool,
}

impl WorkerGenerationSlot {
    pub(super) fn new(slot: Arc<GenerationSlot>) -> Self {
        Self {
            slot,
            released: false,
        }
    }

    pub(crate) fn registration_closed(&self) -> bool {
        self.slot.registration.upgrade().is_none_or(|registration| {
            registration
                .closing
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }

    pub(crate) fn start(&self) -> Result<WorkerSlotUse, SubscriptionWorkerRuntimeError> {
        self.slot.start()
    }

    pub(crate) fn transfer(mut self) {
        self.released = true;
    }
}

impl Drop for WorkerGenerationSlot {
    fn drop(&mut self) {
        if !self.released && self.slot.release().is_err() {
            // A running generation remains registered fail-closed for its concrete owner.
        }
    }
}

pub(crate) struct WorkerSlotUse {
    slot: Arc<GenerationSlot>,
    worker: u64,
    live: bool,
}

impl WorkerSlotUse {
    pub(crate) fn complete(mut self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.slot.complete(self.worker)?;
        self.live = false;
        Ok(())
    }

    pub(in crate::subscription::worker) fn retire(mut self, record: RetiredWorker) {
        self.slot.retire(self.worker, record);
        self.live = false;
    }
}

impl Drop for WorkerSlotUse {
    fn drop(&mut self) {
        if self.live {
            // The concrete owner must retire this use; leave it registered fail-closed.
        }
    }
}
