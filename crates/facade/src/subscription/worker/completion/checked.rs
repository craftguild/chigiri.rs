//! Native join verification before releasing a closed HTTP integrity owner.

use super::*;
use std::sync::atomic::AtomicBool;

impl<T: Send + 'static> ConcreteWorker<T> {
    pub(crate) fn retain_subscription_integrity(
        &mut self,
        activity: crate::managed::EventActivity,
    ) {
        self.join
            .as_mut()
            .expect("live managed subscription worker")
            .integrity = Some(activity);
    }

    #[cfg(test)]
    pub(crate) fn abort_handle_for_test(&self) -> tokio::task::AbortHandle {
        self.join
            .as_ref()
            .and_then(|join| join.handle.as_ref())
            .expect("live test worker")
            .abort_handle()
    }

    #[cfg(test)]
    pub(crate) fn is_finished_for_test(&self) -> bool {
        self.join
            .as_ref()
            .and_then(|join| join.handle.as_ref())
            .is_some_and(tokio::task::JoinHandle::is_finished)
    }

    pub(crate) fn poll_http(
        &mut self,
        context: &mut Context<'_>,
        poisoned: &AtomicBool,
    ) -> Poll<ConcreteWorkerOutcome<T>> {
        self.poll_checked(context, Some(poisoned))
    }

    pub(super) fn poll_checked(
        &mut self,
        context: &mut Context<'_>,
        poisoned: Option<&AtomicBool>,
    ) -> Poll<ConcreteWorkerOutcome<T>> {
        let outcome = match self
            .join
            .as_mut()
            .expect("live concrete worker")
            .poll_typed(context)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        self.finish_checked(outcome, poisoned)
    }

    pub(super) fn finish_checked(
        &mut self,
        outcome: TypedJoinOutcome<T>,
        poisoned: Option<&AtomicBool>,
    ) -> Poll<ConcreteWorkerOutcome<T>> {
        if poisoned.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
            if let TypedJoinOutcome::Completed(value) = outcome {
                let _ = dispose_value(value);
            }
            self.join.as_mut().expect("joined poisoned worker").poisoned = true;
            return Poll::Ready(ConcreteWorkerOutcome::Poisoned);
        }
        match outcome {
            TypedJoinOutcome::Completed(value) => match self.complete_slot() {
                Ok(()) => Poll::Ready(ConcreteWorkerOutcome::Completed(value)),
                Err(()) => {
                    let _ = dispose_value(value);
                    Poll::Ready(ConcreteWorkerOutcome::Poisoned)
                }
            },
            TypedJoinOutcome::Cancelled => {
                if self.complete_slot().is_ok() {
                    Poll::Ready(ConcreteWorkerOutcome::Cancelled)
                } else {
                    Poll::Ready(ConcreteWorkerOutcome::Poisoned)
                }
            }
            TypedJoinOutcome::Panicked => {
                if self.complete_slot().is_ok() {
                    Poll::Ready(ConcreteWorkerOutcome::Panicked)
                } else {
                    Poll::Ready(ConcreteWorkerOutcome::Poisoned)
                }
            }
            TypedJoinOutcome::Poisoned => Poll::Ready(ConcreteWorkerOutcome::Poisoned),
        }
    }
}
