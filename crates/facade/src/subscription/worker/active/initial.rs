//! Sole ownership of the original overall deadline through initial encoding.

use std::task::{Context, Poll};

use chigiri_http::{ResponseCompletionOutcome, ResponseParts};

use super::{ActiveSubscriptionWorker, ActiveWorkerSignal};
use crate::subscription::bridge_resources::HttpSubscriptionPhysicalLease;
use crate::subscription::control::SubscriptionControlSignal;
use crate::subscription::delivery::EventByteCharge;
use crate::subscription::worker::{
    InitialResponseBufferLease, MonitoredDeadlineTable, SubscriptionWorkerRuntimeError,
};

/// Move-only deadline owner held until bounded initial response preparation ends.
pub(crate) struct InitialResponseDeadline {
    owner: Option<InitialResponseOwner>,
    completed: Option<ResponseParts>,
    buffer: Option<InitialResponseBufferLease>,
}

enum InitialResponseOwner {
    Source(ActiveSubscriptionWorker),
    Response(ActiveSubscriptionWorker),
    Finite(MonitoredDeadlineTable),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitialResponseGate {
    Ready,
    TimedOut,
    Failed,
}

impl InitialResponseDeadline {
    pub(crate) fn source_delivery(
        &self,
    ) -> Option<std::sync::Arc<chigiri_runtime::__private::HostSourceDelivery>> {
        match &self.owner {
            Some(InitialResponseOwner::Source(worker)) => worker.source_delivery(),
            _ => None,
        }
    }
    pub(crate) fn task_failed(&self) -> bool {
        matches!(&self.owner, Some(InitialResponseOwner::Source(worker) | InitialResponseOwner::Response(worker)) if worker.activity.task_failed())
    }

    pub(super) const fn source(
        worker: ActiveSubscriptionWorker,
        buffer: Option<InitialResponseBufferLease>,
    ) -> Self {
        Self {
            owner: Some(InitialResponseOwner::Source(worker)),
            completed: None,
            buffer,
        }
    }

    pub(crate) const fn response(
        worker: ActiveSubscriptionWorker,
        buffer: Option<InitialResponseBufferLease>,
    ) -> Self {
        Self {
            owner: Some(InitialResponseOwner::Response(worker)),
            completed: None,
            buffer,
        }
    }

    pub(crate) const fn finite(
        deadlines: MonitoredDeadlineTable,
        buffer: Option<InitialResponseBufferLease>,
    ) -> Self {
        Self {
            owner: Some(InitialResponseOwner::Finite(deadlines)),
            completed: None,
            buffer,
        }
    }

    pub(crate) fn finish_bytes(
        &mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionControlSignal> {
        let charge = self
            .buffer
            .as_mut()
            .ok_or(SubscriptionControlSignal::Accounting)?
            .try_finish(response)?;
        self.buffer.take();
        Ok(charge)
    }

    pub(crate) fn take_buffer(&mut self) -> Option<InitialResponseBufferLease> {
        self.buffer.take()
    }

    /// Issues the same HTTP quota owner before fallible commit or byte transfer.
    pub(crate) fn physical_lease(
        &self,
    ) -> Result<HttpSubscriptionPhysicalLease, SubscriptionWorkerRuntimeError> {
        match &self.owner {
            Some(InitialResponseOwner::Source(worker)) => worker
                .operation
                .as_ref()
                .ok_or(SubscriptionWorkerRuntimeError::Invariant)?
                .http_physical_lease()
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant),
            _ => Err(SubscriptionWorkerRuntimeError::Invariant),
        }
    }

    /// Performs the sole deadline-first linearization immediately before Commit.
    pub(crate) fn poll_before_commit(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<InitialResponseGate> {
        match self.owner.as_mut().expect("live initial response deadline") {
            InitialResponseOwner::Source(worker) | InitialResponseOwner::Response(worker) => {
                match worker.poll_terminal(context) {
                    Poll::Ready(ActiveWorkerSignal::TimedOut) => {
                        Poll::Ready(InitialResponseGate::TimedOut)
                    }
                    Poll::Ready(_) => Poll::Ready(InitialResponseGate::Failed),
                    Poll::Pending if worker.cleanup_in_progress() => Poll::Pending,
                    Poll::Pending => Poll::Ready(InitialResponseGate::Ready),
                }
            }
            InitialResponseOwner::Finite(deadlines) => match deadlines.poll_expired(context) {
                Poll::Ready(Ok(_)) => Poll::Ready(InitialResponseGate::TimedOut),
                Poll::Ready(Err(_)) => Poll::Ready(InitialResponseGate::Failed),
                Poll::Pending => Poll::Ready(InitialResponseGate::Ready),
            },
        }
    }

    pub(crate) fn begin_completion(
        &mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.worker_mut()?
            .begin_initial_completion(response, outcome)
    }

    pub(crate) fn poll_completion(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<ResponseParts, SubscriptionWorkerRuntimeError>> {
        let response_owner = matches!(self.owner, Some(InitialResponseOwner::Response(_)));
        if self.completed.is_none() {
            let response = match self.worker_mut()?.poll_initial_completion(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(response)) => response,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            };
            self.completed = Some(response);
        }
        if response_owner {
            match self.worker_mut()?.poll_join(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(())) => {
                    return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Shutdown));
                }
            }
        }
        Poll::Ready(Ok(self
            .completed
            .take()
            .expect("completed initial response")))
    }

    /// Joins a callback-free response task while the outer deadline stays armed.
    pub(crate) fn poll_response_finished(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        match self.owner.as_mut() {
            Some(InitialResponseOwner::Response(worker)) => match worker.poll_join(context) {
                Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
                Poll::Ready(Err(())) => Poll::Ready(Err(SubscriptionWorkerRuntimeError::Shutdown)),
                Poll::Pending => Poll::Pending,
            },
            Some(InitialResponseOwner::Finite(_)) => Poll::Ready(Ok(())),
            Some(InitialResponseOwner::Source(_)) | None => {
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
            }
        }
    }

    pub(crate) fn commit_source(
        &mut self,
        response: &ResponseParts,
    ) -> Result<(ActiveSubscriptionWorker, EventByteCharge), SubscriptionWorkerRuntimeError> {
        let owner = self.owner.take();
        let Some(InitialResponseOwner::Source(mut worker)) = owner else {
            self.owner = owner;
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        };
        if let Err(error) = worker.commit_initial_response() {
            self.owner = Some(InitialResponseOwner::Source(worker));
            return Err(error);
        }
        match self.finish_bytes(response) {
            Ok(charge) => Ok((worker, charge)),
            Err(_) => {
                self.owner = Some(InitialResponseOwner::Source(worker));
                Err(SubscriptionWorkerRuntimeError::Invariant)
            }
        }
    }

    pub(crate) fn commit_response(
        &mut self,
        response: &ResponseParts,
    ) -> Result<EventByteCharge, SubscriptionWorkerRuntimeError> {
        let mut owner = self.owner.take();
        let result = match &mut owner {
            Some(InitialResponseOwner::Response(worker)) => worker.commit_initial_response(),
            Some(InitialResponseOwner::Finite(deadlines)) => deadlines.commit_initial_response(),
            _ => Err(SubscriptionWorkerRuntimeError::Invariant),
        };
        if let Err(error) = result {
            self.owner = owner;
            return Err(error);
        }
        match self.finish_bytes(response) {
            Ok(charge) => Ok(charge),
            Err(_) => {
                self.owner = owner;
                Err(SubscriptionWorkerRuntimeError::Invariant)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_commit(&self) {
        match &self.owner {
            Some(InitialResponseOwner::Source(worker) | InitialResponseOwner::Response(worker)) => {
                worker
                    .deadlines
                    .as_ref()
                    .expect("live deadline table")
                    .close();
            }
            Some(InitialResponseOwner::Finite(deadlines)) => deadlines.close(),
            None => panic!("live initial owner"),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_shrinks(&mut self, count: usize) {
        self.buffer
            .as_mut()
            .expect("live initial buffer")
            .fail_shrinks(count);
    }

    fn worker_mut(
        &mut self,
    ) -> Result<&mut ActiveSubscriptionWorker, SubscriptionWorkerRuntimeError> {
        match self.owner.as_mut() {
            Some(InitialResponseOwner::Source(worker) | InitialResponseOwner::Response(worker)) => {
                Ok(worker)
            }
            Some(InitialResponseOwner::Finite(_)) | None => {
                Err(SubscriptionWorkerRuntimeError::Invariant)
            }
        }
    }
}
