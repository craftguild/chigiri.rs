//! Phase B fresh-context Query or Mutation execution under one concrete worker.

mod task;

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_runtime::__private::WebSocketOperationKind;
use chigiri_runtime::GraphQlResponse;

use super::{
    JoinedWebSocketOneShot, PreparedWebSocketWorkerOperation, WebSocketActiveStartFailure,
    WebSocketOneShotOutcome, WebSocketTerminalCarrier, WebSocketTerminalMessage,
};
use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeOperationKind, BridgeResourceError, DetachedCleanup,
};
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::outbox::SubscriptionEventReservation;
use crate::subscription::worker::{
    ConcreteWorker, ConcreteWorkerOutcome, MonitoredDeadlineTable, SubscriptionWorkerRuntimeError,
    WorkerRetainedState,
};

type OneShotTaskResult = Result<GraphQlResponse, SubscriptionWorkerRuntimeError>;

pub(crate) struct ActiveWebSocketOneShot {
    operation: Option<ActiveBridgeOperation>,
    worker: Option<ConcreteWorker<OneShotTaskResult>>,
    deadlines: Option<MonitoredDeadlineTable>,
    deadline_factory: &'static dyn chigiri_http::DeadlineFactory,
    reservation: Option<SubscriptionEventReservation>,
    failure: Option<SubscriptionFailureReporter>,
    terminal: Option<Result<WebSocketTerminalCarrier, SubscriptionWorkerRuntimeError>>,
}

#[derive(Clone, Copy)]
enum OneShotTerminalCause {
    TimedOut,
    Failed(SubscriptionWorkerRuntimeError),
    Cancelled,
}

impl<C: Send + Sync + 'static> PreparedWebSocketWorkerOperation<C> {
    pub(crate) fn activate_one_shot(
        self,
        reservation: SubscriptionEventReservation,
    ) -> Result<ActiveWebSocketOneShot, Box<WebSocketActiveStartFailure>> {
        let Self {
            validated,
            operation,
            deadlines,
            mut failure,
        } = self;
        let bridge_kind = match validated.prepared.kind() {
            WebSocketOperationKind::Query => BridgeOperationKind::Query,
            WebSocketOperationKind::Mutation => BridgeOperationKind::Mutation,
            WebSocketOperationKind::Subscription => {
                return Err(Box::new(WebSocketActiveStartFailure::pending_completed(
                    BridgeResourceError::InvalidOperationKind,
                    operation,
                    deadlines,
                    Some(reservation),
                    failure,
                )));
            }
        };
        let (deadlines, armer, factory) = deadlines.into_parts();
        let generation = operation.generation();
        let operation = match operation.activate(bridge_kind) {
            Ok(operation) => operation,
            Err(error) => {
                return Err(Box::new(WebSocketActiveStartFailure::completed(
                    error,
                    generation,
                    deadlines,
                    Some(reservation),
                    failure,
                )));
            }
        };
        let (activity, reporter) = validated.state.task_scope();
        let instrumentation = validated
            .request
            .request()
            .request_telemetry()
            .and_then(chigiri_runtime::RequestTelemetry::instrumentation_scope)
            .cloned();
        let captured = validated
            .state
            .schema
            .capture_instrumentation(&())
            .map(|captured| captured.retain_within(0));
        let armer = armer
            .with_task(activity.clone())
            .with_instrumentation(captured);
        if activity.is_managed() {
            failure.bind_task(activity.clone());
        }
        let work = task::run(validated, armer);
        let spawned = match reporter {
            Some(reporter) => operation.spawn_worker(
                crate::subscription::worker::managed_task::ManagedSubscriptionTask::new(
                    work,
                    activity.clone(),
                    reporter,
                )
                .with_instrumentation(instrumentation),
            ),
            None => operation.spawn_worker(chigiri_runtime::instrumentation::instrument_future(
                instrumentation,
                work,
            )),
        };
        let mut worker = match spawned {
            Ok(worker) => worker,
            Err(error) => {
                return Err(Box::new(WebSocketActiveStartFailure::active(
                    error,
                    operation,
                    deadlines,
                    Some(reservation),
                    failure,
                    factory,
                )));
            }
        };
        if activity.is_managed() {
            worker.retain_subscription_integrity(activity);
        }
        Ok(ActiveWebSocketOneShot {
            operation: Some(operation),
            worker: Some(worker),
            deadlines: Some(deadlines),
            deadline_factory: factory,
            reservation: Some(reservation),
            failure: Some(failure),
            terminal: None,
        })
    }
}

impl ActiveWebSocketOneShot {
    /// Revokes the logical ID now and returns cleanup through the next poll.
    pub(crate) fn cancel(&mut self) {
        self.begin_terminal(OneShotTerminalCause::Cancelled);
    }

    fn begin_terminal(&mut self, cause: OneShotTerminalCause) {
        if self.terminal.is_some() || self.worker.is_none() {
            return;
        }
        let Some(deadlines) = self.deadlines.take() else {
            self.terminal = Some(Err(SubscriptionWorkerRuntimeError::Invariant));
            return;
        };
        let Some(operation) = self.operation.take() else {
            self.terminal = Some(Err(SubscriptionWorkerRuntimeError::Invariant));
            return;
        };
        let worker = self
            .worker
            .take()
            .expect("active WebSocket one-shot worker");
        worker.abort();
        let retained =
            WorkerRetainedState::websocket_active(Some(deadlines), self.reservation.take());
        let cleanup = DetachedCleanup::worker(worker.erase(), retained);
        let message = self.terminal_message(cause);
        self.terminal = Some(
            WebSocketTerminalCarrier::active(operation, message, cleanup, self.deadline_factory)
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant),
        );
    }

    fn finish_terminal(&mut self) -> WebSocketOneShotOutcome {
        match self
            .terminal
            .take()
            .unwrap_or(Err(SubscriptionWorkerRuntimeError::Invariant))
        {
            Ok(terminal) => WebSocketOneShotOutcome::Terminal(terminal),
            Err(error) => WebSocketOneShotOutcome::SharedFailure(error),
        }
    }

    fn finish_joined(
        &mut self,
        response: Result<GraphQlResponse, OneShotTerminalCause>,
    ) -> WebSocketOneShotOutcome {
        let Some(operation) = self.operation.take() else {
            return WebSocketOneShotOutcome::SharedFailure(
                SubscriptionWorkerRuntimeError::Invariant,
            );
        };
        let deadlines = self.deadlines.take();
        let cleanup = DetachedCleanup::new(async move { drop(deadlines) });
        let message = match &response {
            Ok(_) => WebSocketTerminalMessage::Complete,
            Err(cause) => self.terminal_message(*cause),
        };
        let terminal = match WebSocketTerminalCarrier::active(
            operation,
            message,
            cleanup,
            self.deadline_factory,
        ) {
            Ok(terminal) => terminal,
            Err(_) => {
                return WebSocketOneShotOutcome::SharedFailure(
                    SubscriptionWorkerRuntimeError::Invariant,
                );
            }
        };
        match response {
            Ok(response) => WebSocketOneShotOutcome::Ready(JoinedWebSocketOneShot::new(
                self.reservation.take().expect("one-shot event reservation"),
                response,
                terminal,
            )),
            Err(_) => {
                self.reservation.take();
                WebSocketOneShotOutcome::Terminal(terminal)
            }
        }
    }

    fn terminal_message(&mut self, cause: OneShotTerminalCause) -> WebSocketTerminalMessage {
        match cause {
            OneShotTerminalCause::Cancelled => WebSocketTerminalMessage::Cancelled,
            OneShotTerminalCause::TimedOut => {
                let error = SubscriptionWorkerRuntimeError::Shutdown;
                WebSocketTerminalMessage::Error(
                    self.failure
                        .take()
                        .expect("WebSocket failure reporter")
                        .response(
                            crate::subscription::failure::SubscriptionFailureClass::TimedOut,
                            &error,
                        ),
                )
            }
            OneShotTerminalCause::Failed(error) => WebSocketTerminalMessage::Error(
                self.failure
                    .take()
                    .expect("WebSocket failure reporter")
                    .response(super::output::worker_failure_class(error), &error),
            ),
        }
    }
}

impl Future for ActiveWebSocketOneShot {
    type Output = WebSocketOneShotOutcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.terminal.is_some() {
            return Poll::Ready(self.finish_terminal());
        }
        match self
            .deadlines
            .as_mut()
            .expect("active WebSocket deadlines")
            .poll_expired(context)
        {
            Poll::Ready(Ok(_)) => {
                self.begin_terminal(OneShotTerminalCause::TimedOut);
                return Poll::Ready(self.finish_terminal());
            }
            Poll::Ready(Err(error)) => {
                self.begin_terminal(OneShotTerminalCause::Failed(error));
                return Poll::Ready(self.finish_terminal());
            }
            Poll::Pending => {}
        }
        let outcome =
            match Pin::new(self.worker.as_mut().expect("active one-shot worker")).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(outcome) => outcome,
            };
        match outcome {
            ConcreteWorkerOutcome::Completed(Ok(response)) => {
                self.worker.take();
                Poll::Ready(self.finish_joined(Ok(response)))
            }
            ConcreteWorkerOutcome::Completed(Err(error)) => {
                self.worker.take();
                Poll::Ready(self.finish_joined(Err(OneShotTerminalCause::Failed(error))))
            }
            ConcreteWorkerOutcome::Cancelled | ConcreteWorkerOutcome::Panicked => {
                self.worker.take();
                Poll::Ready(self.finish_joined(Err(OneShotTerminalCause::Failed(
                    SubscriptionWorkerRuntimeError::Invariant,
                ))))
            }
            ConcreteWorkerOutcome::Poisoned => {
                self.begin_terminal(OneShotTerminalCause::Failed(
                    SubscriptionWorkerRuntimeError::Poisoned,
                ));
                Poll::Ready(self.finish_terminal())
            }
        }
    }
}

impl Drop for ActiveWebSocketOneShot {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.begin_terminal(OneShotTerminalCause::Cancelled);
        }
    }
}
