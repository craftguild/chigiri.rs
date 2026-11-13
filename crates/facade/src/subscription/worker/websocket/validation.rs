//! Phase A full validation with the original setup deadline outside the task.

use std::pin::Pin;
use std::task::{Context, Poll};

use super::deadlines::MonitoredWebSocketDeadlines;
use super::{
    PreparedWebSocketWorkerOperation, WebSocketSetupDeadline, WebSocketTerminalCarrier,
    WebSocketTerminalMessage, WebSocketValidationOutcome, WebSocketValidationStartFailure,
    output::{WebSocketValidationStartFailureCause, worker_failure_class},
};
use crate::subscription::bridge_resources::{DetachedCleanup, PendingBridgeOperation};
use crate::subscription::driver::input::{WebSocketValidationInput, WebSocketValidationTaskOutput};
use crate::subscription::failure::{SubscriptionFailureClass, SubscriptionFailureReporter};
use crate::subscription::worker::validation::ValidationWorker;
use crate::subscription::worker::{
    ConcreteWorkerOutcome, RetiredWorkerOwner, SubscriptionWorkerRuntimeError, WorkerRetainedState,
};

type ValidationResult<C> = WebSocketValidationTaskOutput<C>;

pub(crate) struct WebSocketValidationWorker<C: Send + Sync + 'static> {
    operation: Option<PendingBridgeOperation>,
    deadlines: Option<MonitoredWebSocketDeadlines>,
    worker: Option<ValidationWorker<ValidationResult<C>>>,
    failure: Option<SubscriptionFailureReporter>,
    terminal: Option<Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>>,
}

#[derive(Clone, Copy)]
enum WebSocketValidationTerminal {
    TimedOut,
    Failed(SubscriptionWorkerRuntimeError),
    Cancelled,
}

impl<C: Send + Sync + 'static> WebSocketValidationWorker<C> {
    #[cfg(test)]
    pub(crate) fn is_finished_for_test(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(ValidationWorker::is_finished_for_test)
    }

    pub(crate) fn new(
        validation: impl Into<WebSocketValidationInput<C>>,
        operation: PendingBridgeOperation,
        deadline: WebSocketSetupDeadline,
        mut failure: SubscriptionFailureReporter,
    ) -> Result<Self, Box<WebSocketValidationStartFailure>> {
        let factory = deadline.factory();
        let deadlines = match deadline.into_monitored() {
            Ok(deadlines) => deadlines,
            Err(error) => {
                return Err(Box::new(WebSocketValidationStartFailure {
                    operation: Some(operation),
                    deadlines: None,
                    factory,
                    cause: WebSocketValidationStartFailureCause::Worker(error),
                    failure: Some(failure),
                }));
            }
        };
        let armer = deadlines.armer();
        let validation = validation.into();
        let (activity, reporter) = validation.worker_scope();
        let instrumentation = validation.instrumentation();
        let armer = armer.with_task(activity.clone());
        if activity.is_managed() {
            failure.bind_task(activity.clone());
        }
        let worker = match ValidationWorker::new(
            &operation,
            async move { validation.prepare(armer).await },
            activity,
            reporter,
            instrumentation,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                return Err(Box::new(WebSocketValidationStartFailure {
                    operation: Some(operation),
                    deadlines: Some(deadlines),
                    factory,
                    cause: WebSocketValidationStartFailureCause::Resource(error),
                    failure: Some(failure),
                }));
            }
        };
        Ok(Self {
            operation: Some(operation),
            deadlines: Some(deadlines),
            worker: Some(worker),
            failure: Some(failure),
            terminal: None,
        })
    }

    pub(crate) fn generation(
        &self,
    ) -> Option<crate::subscription::bridge_resources::BridgeGeneration> {
        self.operation
            .as_ref()
            .map(PendingBridgeOperation::generation)
            .or_else(|| {
                self.terminal
                    .as_ref()
                    .and_then(|terminal| terminal.as_ref().ok())
                    .map(WebSocketTerminalCarrier::generation)
            })
    }

    /// Revokes the logical ID now and keeps this future as the bounded drain.
    pub(crate) fn cancel(&mut self) {
        self.begin_cleanup(WebSocketValidationTerminal::Cancelled);
    }

    fn begin_cleanup(&mut self, terminal: WebSocketValidationTerminal) {
        if self.terminal.is_some() || self.worker.is_none() {
            return;
        }
        let deadlines = self
            .deadlines
            .take()
            .expect("WebSocket validation deadlines");
        let factory = deadlines.factory();
        let operation = match self.operation.take() {
            Some(operation) => operation,
            None => return self.fail_closed_with_deadlines(deadlines),
        };
        let worker = self.worker.take().expect("WebSocket validation worker");
        worker.abort();
        let retained = WorkerRetainedState::websocket_pending(deadlines.into_table());
        let cleanup = DetachedCleanup::worker(worker.erase(), retained);
        let message = self.terminal_message(terminal);
        self.terminal = Some(
            WebSocketTerminalCarrier::pending(operation, message, cleanup, factory)
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant),
        );
    }

    fn fail_closed_with_deadlines(&mut self, deadlines: MonitoredWebSocketDeadlines) {
        let retained = WorkerRetainedState::websocket_pending(deadlines.into_table());
        if let Some(worker) = self.worker.take() {
            worker.abort();
            worker.erase().retire(RetiredWorkerOwner::orphan(retained));
        } else {
            std::mem::forget(retained);
        }
        if let Some(operation) = self.operation.take() {
            std::mem::forget(operation);
        }
        self.terminal = Some(Err(SubscriptionWorkerRuntimeError::Invariant));
    }

    fn finish_terminal(&mut self) -> WebSocketValidationOutcome<C> {
        match self
            .terminal
            .take()
            .unwrap_or(Err(SubscriptionWorkerRuntimeError::Invariant))
        {
            Ok(terminal) => WebSocketValidationOutcome::Terminal(terminal),
            Err(error) => WebSocketValidationOutcome::SharedFailure(error),
        }
    }

    fn finish_joined_terminal(
        &mut self,
        terminal: WebSocketValidationTerminal,
    ) -> WebSocketValidationOutcome<C> {
        let Some(operation) = self.operation.take() else {
            return WebSocketValidationOutcome::SharedFailure(
                SubscriptionWorkerRuntimeError::Invariant,
            );
        };
        let Some(deadlines) = self.deadlines.take() else {
            return WebSocketValidationOutcome::SharedFailure(
                SubscriptionWorkerRuntimeError::Invariant,
            );
        };
        let factory = deadlines.factory();
        let deadlines = Some(deadlines.into_table());
        let cleanup = DetachedCleanup::new(async move { drop(deadlines) });
        let message = self.terminal_message(terminal);
        match WebSocketTerminalCarrier::pending(operation, message, cleanup, factory) {
            Ok(terminal) => WebSocketValidationOutcome::Terminal(terminal),
            Err(_) => {
                WebSocketValidationOutcome::SharedFailure(SubscriptionWorkerRuntimeError::Invariant)
            }
        }
    }

    fn terminal_message(
        &mut self,
        terminal: WebSocketValidationTerminal,
    ) -> WebSocketTerminalMessage {
        match terminal {
            WebSocketValidationTerminal::Cancelled => WebSocketTerminalMessage::Cancelled,
            WebSocketValidationTerminal::TimedOut => {
                let error = SubscriptionWorkerRuntimeError::Shutdown;
                WebSocketTerminalMessage::Error(
                    self.failure
                        .take()
                        .expect("WebSocket failure reporter")
                        .response(SubscriptionFailureClass::TimedOut, &error),
                )
            }
            WebSocketValidationTerminal::Failed(error) => WebSocketTerminalMessage::Error(
                self.failure
                    .take()
                    .expect("WebSocket failure reporter")
                    .response(worker_failure_class(error), &error),
            ),
        }
    }
}

impl<C: Send + Sync + 'static> Future for WebSocketValidationWorker<C> {
    type Output = WebSocketValidationOutcome<C>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.terminal.is_some() {
            return Poll::Ready(self.finish_terminal());
        }
        match self
            .deadlines
            .as_mut()
            .expect("WebSocket validation deadlines")
            .poll_expired(context)
        {
            Poll::Ready(Ok(())) => {
                self.begin_cleanup(WebSocketValidationTerminal::TimedOut);
                return Poll::Ready(self.finish_terminal());
            }
            Poll::Ready(Err(error)) => {
                self.begin_cleanup(WebSocketValidationTerminal::Failed(error));
                return Poll::Ready(self.finish_terminal());
            }
            Poll::Pending => {}
        }
        let outcome = match Pin::new(self.worker.as_mut().expect("WebSocket validation worker"))
            .poll(context)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        match outcome {
            ConcreteWorkerOutcome::Completed(Ok(WebSocketValidationTaskOutput::Prepared(
                validated,
            ))) => {
                self.worker.take();
                Poll::Ready(WebSocketValidationOutcome::Prepared(Box::new(
                    PreparedWebSocketWorkerOperation {
                        validated: *validated,
                        operation: self.operation.take().expect("pending WebSocket operation"),
                        deadlines: self
                            .deadlines
                            .take()
                            .expect("WebSocket monitored deadlines"),
                        failure: self.failure.take().expect("WebSocket failure reporter"),
                    },
                )))
            }
            ConcreteWorkerOutcome::Completed(Ok(WebSocketValidationTaskOutput::Response(
                response,
            ))) => {
                self.worker.take();
                let Some(operation) = self.operation.take() else {
                    return Poll::Ready(WebSocketValidationOutcome::SharedFailure(
                        SubscriptionWorkerRuntimeError::Invariant,
                    ));
                };
                let Some(deadlines) = self.deadlines.take() else {
                    return Poll::Ready(WebSocketValidationOutcome::SharedFailure(
                        SubscriptionWorkerRuntimeError::Invariant,
                    ));
                };
                let factory = deadlines.factory();
                let deadlines = deadlines.into_table();
                let cleanup = DetachedCleanup::new(async move { drop(deadlines) });
                let terminal = WebSocketTerminalCarrier::pending(
                    operation,
                    WebSocketTerminalMessage::Error(response),
                    cleanup,
                    factory,
                )
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant);
                self.terminal = Some(terminal);
                Poll::Ready(self.finish_terminal())
            }
            ConcreteWorkerOutcome::Completed(Err(error)) => {
                self.worker.take();
                let terminal = if error == SubscriptionWorkerRuntimeError::EndpointClosed {
                    WebSocketValidationTerminal::Cancelled
                } else {
                    WebSocketValidationTerminal::Failed(error)
                };
                Poll::Ready(self.finish_joined_terminal(terminal))
            }
            ConcreteWorkerOutcome::Cancelled | ConcreteWorkerOutcome::Panicked => {
                self.worker.take();
                Poll::Ready(
                    self.finish_joined_terminal(WebSocketValidationTerminal::Failed(
                        SubscriptionWorkerRuntimeError::Invariant,
                    )),
                )
            }
            ConcreteWorkerOutcome::Poisoned => {
                self.begin_cleanup(WebSocketValidationTerminal::Failed(
                    SubscriptionWorkerRuntimeError::Poisoned,
                ));
                Poll::Ready(self.finish_terminal())
            }
        }
    }
}

impl<C: Send + Sync + 'static> Drop for WebSocketValidationWorker<C> {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.begin_cleanup(WebSocketValidationTerminal::Cancelled);
        }
    }
}
