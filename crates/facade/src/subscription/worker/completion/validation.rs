//! Dispose rejected SDK-owned validation output before completing its slot.

use super::*;
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::validation::handoff::Receiver;

impl ConcreteWorker<Result<(), SubscriptionWorkerRuntimeError>> {
    pub(in crate::subscription::worker) fn poll_validation<T>(
        &mut self,
        context: &mut Context<'_>,
        receiver: &mut Receiver<T>,
    ) -> Poll<ConcreteWorkerOutcome<Result<(), SubscriptionWorkerRuntimeError>>> {
        let outcome = match self
            .join
            .as_mut()
            .expect("live validation join")
            .poll_typed(context)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        if !matches!(outcome, TypedJoinOutcome::Completed(Ok(())))
            || receiver.activity().task_failed()
        {
            receiver.close();
        }
        receiver.finish_observation();
        if receiver.activity().task_poisoned() {
            self.join
                .as_mut()
                .expect("joined validation worker")
                .poisoned = true;
            return Poll::Ready(ConcreteWorkerOutcome::Poisoned);
        }
        self.finish_checked(outcome, None)
    }
}
