//! Closed validation handoff; native join results never contain application data.

pub(super) mod handoff;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{
    ConcreteWorker, ConcreteWorkerOutcome, ErasedWorkerCompletion, SubscriptionWorkerRuntimeError,
};
use crate::managed::{EventActivity, HttpFaultReporter};
use crate::subscription::bridge_resources::{
    BridgeConnection, BridgeResourceError, PendingBridgeOperation,
};

pub(super) struct ValidationWorker<T: Send + 'static> {
    worker: Option<ConcreteWorker<Result<(), SubscriptionWorkerRuntimeError>>>,
    receiver: handoff::Receiver<T>,
}

impl<T: Send + 'static> ValidationWorker<T> {
    pub(super) fn connection(
        connection: &BridgeConnection,
        future: impl Future<Output = Result<T, SubscriptionWorkerRuntimeError>> + Send + 'static,
        activity: EventActivity,
        reporter: HttpFaultReporter,
        instrumentation: Option<chigiri_runtime::instrumentation::InstrumentationScope>,
    ) -> Result<Self, BridgeResourceError> {
        let (sender, receiver) = handoff::channel(activity.clone(), Some(reporter.clone()));
        let work = async move { sender.offer(future.await?).await };
        let mut worker = connection.spawn_worker(
            super::managed_task::ManagedSubscriptionTask::new(work, activity.clone(), reporter)
                .with_instrumentation(instrumentation),
        )?;
        worker.retain_subscription_integrity(activity);
        Ok(Self {
            worker: Some(worker),
            receiver,
        })
    }

    pub(super) fn new(
        operation: &PendingBridgeOperation,
        future: impl Future<Output = Result<T, SubscriptionWorkerRuntimeError>> + Send + 'static,
        activity: EventActivity,
        reporter: Option<HttpFaultReporter>,
        instrumentation: Option<chigiri_runtime::instrumentation::InstrumentationScope>,
    ) -> Result<Self, BridgeResourceError> {
        let (sender, receiver) = handoff::channel(activity.clone(), reporter.clone());
        let work = async move { sender.offer(future.await?).await };
        let mut worker = match reporter {
            Some(reporter) => operation.spawn_worker(
                super::managed_task::ManagedSubscriptionTask::new(work, activity.clone(), reporter)
                    .with_instrumentation(instrumentation),
            ),
            None => operation.spawn_worker(chigiri_runtime::instrumentation::instrument_future(
                instrumentation,
                work,
            )),
        }?;
        worker.retain_subscription_integrity(activity);
        Ok(Self {
            worker: Some(worker),
            receiver,
        })
    }

    pub(super) fn abort(&self) {
        self.worker
            .as_ref()
            .expect("live validation worker")
            .abort();
    }

    pub(super) fn erase(mut self) -> ErasedWorkerCompletion {
        self.receiver.close();
        self.worker.take().expect("live validation worker").erase()
    }

    #[cfg(test)]
    pub(super) fn abort_handle_for_test(&self) -> tokio::task::AbortHandle {
        self.worker
            .as_ref()
            .expect("live validation worker")
            .abort_handle_for_test()
    }

    #[cfg(test)]
    pub(super) fn is_finished_for_test(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(ConcreteWorker::is_finished_for_test)
    }
}

impl<T: Send + 'static> Future for ValidationWorker<T> {
    type Output = ConcreteWorkerOutcome<Result<T, SubscriptionWorkerRuntimeError>>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // Receipt transfers ownership, but only an actual join may release the
        // original worker slot or let the caller activate the prepared result.
        let _ = self.receiver.poll_accept(context);
        let this = &mut *self;
        match this
            .worker
            .as_mut()
            .expect("live validation worker")
            .poll_validation(context, &mut this.receiver)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(ConcreteWorkerOutcome::Completed(Ok(()))) => match this.receiver.take() {
                Some(output) => Poll::Ready(ConcreteWorkerOutcome::Completed(Ok(output))),
                None => Poll::Ready(ConcreteWorkerOutcome::Panicked),
            },
            Poll::Ready(ConcreteWorkerOutcome::Poisoned) => {
                Poll::Ready(ConcreteWorkerOutcome::Poisoned)
            }
            Poll::Ready(ConcreteWorkerOutcome::Cancelled) => {
                Poll::Ready(ConcreteWorkerOutcome::Cancelled)
            }
            Poll::Ready(ConcreteWorkerOutcome::Completed(Err(error))) => {
                Poll::Ready(ConcreteWorkerOutcome::Completed(Err(error)))
            }
            Poll::Ready(ConcreteWorkerOutcome::Panicked) => {
                Poll::Ready(ConcreteWorkerOutcome::Panicked)
            }
        }
    }
}

impl<T: Send + 'static> Drop for ValidationWorker<T> {
    fn drop(&mut self) {
        self.receiver.close();
    }
}
