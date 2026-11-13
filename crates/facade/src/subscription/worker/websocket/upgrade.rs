//! Native conversion uses the original connection slot before authorization.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::__streaming::{WebSocketIo, WebSocketIoError};
use crate::BoxFuture;
use crate::managed::{EventActivity, HttpFaultReporter};
use crate::subscription::bridge_resources::{BridgeConnection, BridgeResourceError};
use crate::subscription::worker::{
    ConcreteWorkerOutcome, ErasedWorkerCompletion, SubscriptionWorkerRuntimeError, TaskOwnedFuture,
    TaskOwnedValue, validation::ValidationWorker,
};

pub(crate) struct NativeUpgradeWorker<S: WebSocketIo> {
    worker: Option<ValidationWorker<Result<S, WebSocketIoError>>>,
}

impl<S: WebSocketIo> NativeUpgradeWorker<S> {
    pub(crate) fn new(
        connection: &BridgeConnection,
        socket: BoxFuture<'static, Result<S, WebSocketIoError>>,
        activity: EventActivity,
        reporter: HttpFaultReporter,
        instrumentation: Option<chigiri_runtime::instrumentation::InstrumentationScope>,
    ) -> Result<Self, BridgeResourceError> {
        // Construct the guard before spawning, including cancellation before
        // the first poll. Ready-future destruction precedes value transfer.
        let mut socket = TaskOwnedFuture::new(socket, Some(activity.clone()));
        let task_activity = activity.clone();
        let work = async move {
            let output = tokio::select! {
                biased;
                _ = task_activity.source_stopped() => return Err(SubscriptionWorkerRuntimeError::EndpointClosed),
                output = &mut socket => TaskOwnedValue::new(output, Some(task_activity.clone())),
            };
            drop(socket);
            if task_activity.task_failed() || matches!(&*output, Err(WebSocketIoError::Invariant)) {
                Err(SubscriptionWorkerRuntimeError::Invariant)
            } else {
                Ok(output.into_inner())
            }
        };
        Ok(Self {
            worker: Some(ValidationWorker::connection(
                connection,
                work,
                activity,
                reporter,
                instrumentation,
            )?),
        })
    }

    pub(crate) fn retire(mut self) -> Option<ErasedWorkerCompletion> {
        self.worker.take().map(|worker| {
            worker.abort();
            worker.erase()
        })
    }
}

impl<S: WebSocketIo> Future for NativeUpgradeWorker<S> {
    type Output =
        ConcreteWorkerOutcome<Result<Result<S, WebSocketIoError>, SubscriptionWorkerRuntimeError>>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let outcome =
            match Pin::new(self.worker.as_mut().expect("live native upgrade")).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(outcome) => outcome,
            };
        if !matches!(outcome, ConcreteWorkerOutcome::Poisoned) {
            self.worker.take();
        }
        Poll::Ready(outcome)
    }
}
