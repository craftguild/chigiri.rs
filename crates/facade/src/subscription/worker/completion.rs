//! Concrete Tokio completion ownership without detach-on-drop handles.

mod checked;
mod retained;
mod validation;

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::task::JoinHandle;

use super::registration::WorkerSlotUse;
pub(crate) use retained::{RetiredWorkerOwner, WorkerRetainedState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerCompletionOutcome {
    Completed,
    Poisoned,
}

pub(crate) enum ConcreteWorkerOutcome<T> {
    Completed(T),
    Cancelled,
    Panicked,
    Poisoned,
}

enum TypedJoinOutcome<T> {
    Completed(T),
    Cancelled,
    Panicked,
    Poisoned,
}

struct TypedJoin<T> {
    handle: Option<JoinHandle<T>>,
    poisoned: bool,
    integrity: Option<crate::managed::EventActivity>,
}

impl<T: Send + 'static> TypedJoin<T> {
    fn new(handle: JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
            poisoned: false,
            integrity: None,
        }
    }

    fn abort(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }

    fn poll_typed(&mut self, context: &mut Context<'_>) -> Poll<TypedJoinOutcome<T>> {
        if self.poisoned {
            return Poll::Ready(TypedJoinOutcome::Poisoned);
        }
        let Some(handle) = self.handle.as_mut() else {
            return Poll::Ready(TypedJoinOutcome::Cancelled);
        };
        let result = match Pin::new(handle).poll(context) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };
        drop(self.handle.take());
        let outcome = match result {
            Ok(value) => TypedJoinOutcome::Completed(value),
            Err(error) if error.is_panic() => {
                if dispose_payload(error.into_panic()) {
                    TypedJoinOutcome::Panicked
                } else {
                    self.poisoned = true;
                    TypedJoinOutcome::Poisoned
                }
            }
            Err(error) => {
                drop(error);
                TypedJoinOutcome::Cancelled
            }
        };
        if self
            .integrity
            .as_ref()
            .is_some_and(crate::managed::EventActivity::task_poisoned)
        {
            if let TypedJoinOutcome::Completed(value) = outcome {
                let _ = dispose_value(value);
            }
            self.poisoned = true;
            return Poll::Ready(TypedJoinOutcome::Poisoned);
        }
        Poll::Ready(outcome)
    }
}

trait ErasedJoin: Send {
    fn abort(&self);
    fn poll_erased(&mut self, context: &mut Context<'_>) -> Poll<WorkerCompletionOutcome>;
    #[cfg(test)]
    fn native_state_for_test(&self) -> (Option<bool>, bool);
}

impl<T: Send + 'static> ErasedJoin for TypedJoin<T> {
    #[cfg(test)]
    fn native_state_for_test(&self) -> (Option<bool>, bool) {
        (
            self.handle.as_ref().map(JoinHandle::is_finished),
            self.poisoned,
        )
    }

    fn abort(&self) {
        Self::abort(self);
    }

    fn poll_erased(&mut self, context: &mut Context<'_>) -> Poll<WorkerCompletionOutcome> {
        match self.poll_typed(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(TypedJoinOutcome::Completed(value)) => {
                if dispose_value(value) {
                    Poll::Ready(WorkerCompletionOutcome::Completed)
                } else {
                    Poll::Ready(WorkerCompletionOutcome::Poisoned)
                }
            }
            Poll::Ready(TypedJoinOutcome::Cancelled | TypedJoinOutcome::Panicked) => {
                Poll::Ready(WorkerCompletionOutcome::Completed)
            }
            Poll::Ready(TypedJoinOutcome::Poisoned) => {
                Poll::Ready(WorkerCompletionOutcome::Poisoned)
            }
        }
    }
}

/// Typed outer-worker ownership before a terminal path erases the task output.
pub(crate) struct ConcreteWorker<T: Send + 'static> {
    join: Option<TypedJoin<T>>,
    slot: Option<WorkerSlotUse>,
}

impl<T: Send + 'static> ConcreteWorker<T> {
    pub(super) fn new(handle: JoinHandle<T>, slot: WorkerSlotUse) -> Self {
        Self {
            join: Some(TypedJoin::new(handle)),
            slot: Some(slot),
        }
    }

    pub(crate) fn abort(&self) {
        if let Some(join) = &self.join {
            join.abort();
        }
    }

    pub(crate) fn erase(mut self) -> ErasedWorkerCompletion {
        ErasedWorkerCompletion {
            join: Some(Box::new(self.join.take().expect("live concrete worker"))),
            slot: self.slot.take(),
        }
    }
}

impl<T: Send + 'static> Future for ConcreteWorker<T> {
    type Output = ConcreteWorkerOutcome<T>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_checked(context, None)
    }
}

impl<T: Send + 'static> ConcreteWorker<T> {
    fn complete_slot(&mut self) -> Result<(), ()> {
        self.join.take();
        self.slot
            .take()
            .expect("live worker slot")
            .complete()
            .map_err(|_| ())
    }
}

impl<T: Send + 'static> Drop for ConcreteWorker<T> {
    fn drop(&mut self) {
        let (Some(join), Some(slot)) = (self.join.take(), self.slot.take()) else {
            return;
        };
        join.abort();
        slot.retire(RetiredWorker::new(
            Box::new(join),
            RetiredWorkerOwner::orphan(WorkerRetainedState::empty()),
        ));
    }
}

/// Erased output owner used only after logical terminal revocation.
pub(crate) struct ErasedWorkerCompletion {
    join: Option<Box<dyn ErasedJoin>>,
    slot: Option<WorkerSlotUse>,
}

impl ErasedWorkerCompletion {
    pub(crate) const fn is_live(&self) -> bool {
        self.join.is_some() && self.slot.is_some()
    }

    pub(crate) fn abort(&self) {
        if let Some(join) = &self.join {
            join.abort();
        }
    }

    pub(crate) fn retire(mut self, owner: RetiredWorkerOwner) {
        let join = self.join.take().expect("live erased worker");
        let slot = self.slot.take().expect("live worker slot");
        slot.retire(RetiredWorker::new(join, owner));
    }
}

impl Future for ErasedWorkerCompletion {
    type Output = WorkerCompletionOutcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let outcome = match self
            .join
            .as_mut()
            .expect("live erased worker")
            .poll_erased(context)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        if outcome == WorkerCompletionOutcome::Completed {
            self.join.take();
            if self
                .slot
                .take()
                .expect("live worker slot")
                .complete()
                .is_err()
            {
                return Poll::Ready(WorkerCompletionOutcome::Poisoned);
            }
        }
        Poll::Ready(outcome)
    }
}

impl Drop for ErasedWorkerCompletion {
    fn drop(&mut self) {
        let (Some(join), Some(slot)) = (self.join.take(), self.slot.take()) else {
            return;
        };
        join.abort();
        slot.retire(RetiredWorker::new(
            join,
            RetiredWorkerOwner::orphan(WorkerRetainedState::empty()),
        ));
    }
}

pub(super) struct RetiredWorker {
    join: Option<Box<dyn ErasedJoin>>,
    owner: Option<RetiredWorkerOwner>,
}

impl RetiredWorker {
    #[cfg(test)]
    pub(super) fn native_state_for_test(&self) -> String {
        format!(
            "join(finished,poisoned)={:?}, owner_poisoned={:?}",
            self.join.as_ref().map(|join| join.native_state_for_test()),
            self.owner.as_ref().map(RetiredWorkerOwner::is_poisoned)
        )
    }

    fn new(join: Box<dyn ErasedJoin>, owner: RetiredWorkerOwner) -> Self {
        Self {
            join: Some(join),
            owner: Some(owner),
        }
    }

    pub(super) fn poll(&mut self, context: &mut Context<'_>) -> Poll<WorkerCompletionOutcome> {
        let result = self
            .join
            .as_mut()
            .expect("retired worker join")
            .poll_erased(context);
        if result == Poll::Ready(WorkerCompletionOutcome::Completed)
            && self
                .owner
                .as_ref()
                .is_some_and(RetiredWorkerOwner::is_poisoned)
        {
            Poll::Ready(WorkerCompletionOutcome::Poisoned)
        } else {
            result
        }
    }

    pub(super) fn finish(mut self) -> bool {
        let join = self.join.take().expect("retired worker join");
        if !dispose_value(join) {
            std::mem::forget(self.owner.take());
            return false;
        }
        self.owner.take().expect("retired worker owner").finish()
    }
}

impl Drop for RetiredWorker {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            std::mem::forget(join);
        }
        if let Some(owner) = self.owner.take() {
            std::mem::forget(owner);
        }
    }
}

pub(crate) fn dispose_value<T>(value: T) -> bool {
    dispose_caught(catch_unwind(AssertUnwindSafe(|| drop(value))))
}

pub(crate) fn dispose_payload(payload: Box<dyn Any + Send + 'static>) -> bool {
    dispose_caught(catch_unwind(AssertUnwindSafe(|| drop(payload))))
}

fn dispose_caught(result: std::thread::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(nested) => {
            if let Err(last) = catch_unwind(AssertUnwindSafe(|| drop(nested))) {
                std::mem::forget(last);
            }
            false
        }
    }
}
