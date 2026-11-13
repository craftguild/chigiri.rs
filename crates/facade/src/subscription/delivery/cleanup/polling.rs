//! Unwind-safe polling for endpoint-owned quarantined cleanup.

use std::any::Any;
use std::future::Future;
use std::ops::Bound::{Excluded, Unbounded};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{CleanupSlot, QuarantineError, QuarantineKey, QuarantineRegistry};

use super::CleanupPhysicalCharge;
#[cfg(feature = "subscription-worker")]
use crate::subscription::worker::{
    ErasedWorkerCompletion, RetiredWorkerOwner, WorkerCompletionOutcome, WorkerGenerationSlot,
    WorkerRetainedState,
};

pub(super) struct OwnedQuarantinedWork {
    work: QuarantinedKind,
    _charges: Option<QuarantinedCharges>,
    #[cfg(feature = "subscription-worker")]
    worker_slot: Option<WorkerGenerationSlot>,
    #[cfg(feature = "subscription-worker")]
    retained: Option<WorkerRetainedState>,
}

enum QuarantinedCharges {
    Erased {
        _owner: Box<dyn Any + Send + 'static>,
    },
    #[cfg(feature = "subscription-worker")]
    Worker(CleanupPhysicalCharge),
}

enum QuarantinedKind {
    Inline(Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>),
    #[cfg(feature = "subscription-worker")]
    Worker(Option<ErasedWorkerCompletion>),
    Completed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WorkOutcome {
    Completed,
    #[cfg(feature = "subscription-worker")]
    Poisoned,
}

impl OwnedQuarantinedWork {
    #[cfg(feature = "subscription-worker")]
    pub(super) fn needs_worker_handoff(&self) -> bool {
        matches!(
            self.work,
            QuarantinedKind::Worker(_) | QuarantinedKind::Completed
        ) && self
            .worker_slot
            .as_ref()
            .is_some_and(WorkerGenerationSlot::registration_closed)
    }

    pub(super) fn new(
        future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
        charges: Box<dyn Any + Send + 'static>,
    ) -> Self {
        Self {
            work: QuarantinedKind::Inline(Some(future)),
            _charges: Some(QuarantinedCharges::Erased { _owner: charges }),
            #[cfg(feature = "subscription-worker")]
            worker_slot: None,
            #[cfg(feature = "subscription-worker")]
            retained: None,
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn worker(
        completion: ErasedWorkerCompletion,
        charges: CleanupPhysicalCharge,
        retained: WorkerRetainedState,
        worker_slot: WorkerGenerationSlot,
    ) -> Self {
        Self {
            work: QuarantinedKind::Worker(Some(completion)),
            _charges: Some(QuarantinedCharges::Worker(charges)),
            worker_slot: Some(worker_slot),
            retained: Some(retained),
        }
    }

    pub(super) fn completed(charges: CleanupPhysicalCharge) -> Self {
        Self {
            work: QuarantinedKind::Completed,
            _charges: Some(QuarantinedCharges::Erased {
                _owner: Box::new(charges),
            }),
            #[cfg(feature = "subscription-worker")]
            worker_slot: None,
            #[cfg(feature = "subscription-worker")]
            retained: None,
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn with_completed_slot(mut self, slot: Option<WorkerGenerationSlot>) -> Self {
        self.worker_slot = slot;
        self
    }

    fn poll(&mut self, context: &mut Context<'_>) -> Poll<WorkOutcome> {
        match &mut self.work {
            QuarantinedKind::Inline(future) => {
                future
                    .as_mut()
                    .map_or(Poll::Ready(WorkOutcome::Completed), |future| {
                        future
                            .as_mut()
                            .poll(context)
                            .map(|()| WorkOutcome::Completed)
                    })
            }
            #[cfg(feature = "subscription-worker")]
            QuarantinedKind::Worker(completion) => {
                let outcome =
                    Pin::new(completion.as_mut().expect("worker completion")).poll(context);
                outcome.map(|outcome| match outcome {
                    WorkerCompletionOutcome::Completed => WorkOutcome::Completed,
                    WorkerCompletionOutcome::Poisoned => WorkOutcome::Poisoned,
                })
            }
            QuarantinedKind::Completed => Poll::Ready(WorkOutcome::Completed),
        }
    }

    fn drop_completed_future(&mut self) -> std::thread::Result<()> {
        let work = std::mem::replace(&mut self.work, QuarantinedKind::Completed);
        match work {
            QuarantinedKind::Inline(Some(future)) => {
                catch_unwind(AssertUnwindSafe(|| drop(future)))
            }
            QuarantinedKind::Inline(None) | QuarantinedKind::Completed => Ok(()),
            #[cfg(feature = "subscription-worker")]
            QuarantinedKind::Worker(completion) => {
                catch_unwind(AssertUnwindSafe(|| drop(completion)))
            }
        }
    }
}

impl Drop for OwnedQuarantinedWork {
    fn drop(&mut self) {
        #[cfg(feature = "subscription-worker")]
        if let QuarantinedKind::Worker(completion) = &mut self.work {
            let owners = (
                completion.take(),
                self._charges.take(),
                self.retained.take(),
                self.worker_slot.take(),
            );
            if let (
                Some(completion),
                Some(QuarantinedCharges::Worker(charges)),
                Some(retained),
                Some(slot),
            ) = owners
            {
                slot.transfer();
                completion.abort();
                completion.retire(RetiredWorkerOwner::new(charges, retained));
            } else {
                std::mem::forget(owners);
            }
        }
    }
}

struct PollOwnershipGuard {
    registry: QuarantineRegistry,
    key: QuarantineKey,
    work: Option<OwnedQuarantinedWork>,
}

impl PollOwnershipGuard {
    fn new(registry: QuarantineRegistry, key: QuarantineKey, work: OwnedQuarantinedWork) -> Self {
        Self {
            registry,
            key,
            work: Some(work),
        }
    }

    fn poll(&mut self, context: &mut Context<'_>) -> Poll<WorkOutcome> {
        self.work.as_mut().expect("poll ownership").poll(context)
    }

    fn into_work(mut self) -> OwnedQuarantinedWork {
        self.work.take().expect("poll ownership")
    }
}

impl Drop for PollOwnershipGuard {
    fn drop(&mut self) {
        let Some(work) = self.work.take() else {
            return;
        };
        restore_polling_slot(&self.registry, self.key.clone(), work, true);
    }
}

impl QuarantineRegistry {
    /// Polls one generation outside the registry lock.
    pub(crate) fn poll_one(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<QuarantineKey>, QuarantineError>> {
        let (key, work) = match self.take_next_work() {
            Ok(Some(work)) => work,
            Ok(None) => return Poll::Ready(Ok(None)),
            Err(error) => return Poll::Ready(Err(error)),
        };
        let mut guard = PollOwnershipGuard::new(self.clone(), key.clone(), work);
        let result = guard.poll(context);
        let mut work = guard.into_work();

        match result {
            Poll::Ready(WorkOutcome::Completed) => {
                if let Err(payload) = work.drop_completed_future() {
                    restore_polling_slot(self, key, work, false);
                    resume_unwind(payload);
                }
                self.finish_ready(key, work)
            }
            #[cfg(feature = "subscription-worker")]
            Poll::Ready(WorkOutcome::Poisoned) => {
                restore_polling_slot(self, key, work, true);
                Poll::Ready(Err(QuarantineError::Invariant))
            }
            Poll::Pending => {
                let corrupted = restore_polling_slot(self, key, work, false);
                if corrupted {
                    Poll::Ready(Err(QuarantineError::Invariant))
                } else {
                    Poll::Pending
                }
            }
        }
    }

    fn take_next_work(
        &self,
    ) -> Result<Option<(QuarantineKey, OwnedQuarantinedWork)>, QuarantineError> {
        let mut state = self.lock()?;
        let next = state.cursor.as_ref().and_then(|cursor| {
            state
                .slots
                .range((Excluded(cursor), Unbounded))
                .find(|(_, slot)| matches!(slot, CleanupSlot::Quarantined(_)))
                .map(|(key, _)| key.clone())
        });
        let Some(key) = next.or_else(|| {
            state
                .slots
                .iter()
                .find(|(_, slot)| matches!(slot, CleanupSlot::Quarantined(_)))
                .map(|(key, _)| key.clone())
        }) else {
            return Ok(None);
        };
        let Some(slot) = state.slots.get_mut(&key) else {
            state.corrupted = true;
            return Err(QuarantineError::Invariant);
        };
        let CleanupSlot::Quarantined(work) = std::mem::replace(slot, CleanupSlot::Polling) else {
            state.corrupted = true;
            return Err(QuarantineError::Invariant);
        };
        Ok(Some((key, work)))
    }

    fn finish_ready(
        &self,
        key: QuarantineKey,
        work: OwnedQuarantinedWork,
    ) -> Poll<Result<Option<QuarantineKey>, QuarantineError>> {
        let (mut state, lock_corrupted) = self.lock_recover();
        if !matches!(state.slots.get(&key), Some(CleanupSlot::Polling)) {
            state.corrupted = true;
            drop(state);
            std::mem::forget(work);
            return Poll::Ready(Err(QuarantineError::Invariant));
        }
        state.cursor = Some(key.clone());
        state.slots.remove(&key);
        drop(state);
        drop(work);
        if lock_corrupted {
            Poll::Ready(Err(QuarantineError::Invariant))
        } else {
            Poll::Ready(Ok(Some(key)))
        }
    }
}

fn restore_polling_slot(
    registry: &QuarantineRegistry,
    key: QuarantineKey,
    work: OwnedQuarantinedWork,
    poison: bool,
) -> bool {
    let (mut state, lock_corrupted) = registry.lock_recover();
    let Some(slot) = state.slots.get_mut(&key) else {
        state.corrupted = true;
        drop(state);
        std::mem::forget(work);
        return true;
    };
    if !matches!(slot, CleanupSlot::Polling) {
        state.corrupted = true;
        drop(state);
        std::mem::forget(work);
        return true;
    }
    *slot = if poison {
        CleanupSlot::Poisoned { _work: work }
    } else {
        CleanupSlot::Quarantined(work)
    };
    state.cursor = Some(key);
    state.corrupted |= poison || lock_corrupted;
    let corrupted = state.corrupted;
    drop(state);
    // Detach may have scanned while this exact entry was Polling. Recheck
    // after restoring its native owner, including a poisoned completion.
    #[cfg(feature = "subscription-worker")]
    registry.handoff_closed_workers();
    corrupted
}
