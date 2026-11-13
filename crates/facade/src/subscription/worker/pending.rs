//! Phase A validation task with original deadlines outside the concrete worker.

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::SubscriptionTransport;

use super::validation::ValidationWorker;
use super::{
    ConcreteWorkerOutcome, InitialResponseBufferLease, InitialResponseDeadline,
    InitialWorkerResources, RetiredWorkerOwner, SubscriptionWorkerRuntimeError,
    WorkerRetainedState,
};
use crate::subscription::bridge_resources::{
    BridgeCleanupCoordinator, DetachedCleanup, PendingBridgeOperation,
};
use crate::subscription::driver::SubscriptionInitialization;
use crate::subscription::driver::input::{
    PreparedEndpointSubscription, SubscriptionPreparation, ValidatedEndpointSubscription,
};
use crate::subscription::driver::setup_deadlines::SubscriptionSetupDeadlines;
use crate::transport::streaming::InitialRequestObservation;

type ValidationResult<C> =
    Result<ValidatedEndpointSubscription<C>, Box<SubscriptionInitialization<C>>>;

pub(crate) struct PendingPreparationWorker<C: Send + Sync + 'static> {
    operation: Option<PendingBridgeOperation>,
    observation: Option<InitialRequestObservation<C>>,
    transport: SubscriptionTransport,
    timers: Option<SubscriptionSetupDeadlines<'static>>,
    worker: Option<ValidationWorker<ValidationResult<C>>>,
    activity: crate::managed::EventActivity,
    initial: Option<InitialWorkerResources>,
    terminal_buffer: Option<InitialResponseBufferLease>,
    cleanup: Option<BridgeCleanupCoordinator<'static>>,
    terminal: Option<PendingTerminal>,
}

enum PendingTerminal {
    TimedOut,
    Failed,
}

pub(crate) struct PreparedWorkerSubscription<C: Send + Sync + 'static> {
    pub(super) prepared: PreparedEndpointSubscription<'static, C>,
    pub(super) operation: PendingBridgeOperation,
    pub(super) observation: InitialRequestObservation<C>,
    pub(super) transport: SubscriptionTransport,
    pub(super) initial: Option<InitialWorkerResources>,
}

pub(crate) struct PendingPreparationStartFailure<C: Send + Sync + 'static> {
    pub(crate) observation: InitialRequestObservation<C>,
    pub(crate) deadline: Option<InitialResponseDeadline>,
    pub(crate) buffer: Option<InitialResponseBufferLease>,
}

pub(crate) enum PendingPreparationOutcome<C: Send + Sync + 'static> {
    ObservedFailure {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Cancelled {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Prepared(Box<PreparedWorkerSubscription<C>>),
    Response {
        deadline: Box<InitialResponseDeadline>,
        initialization: Box<SubscriptionInitialization<C>>,
        observation: InitialRequestObservation<C>,
    },
    TimedOut {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Failed {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
}

impl<C: Send + Sync + 'static> PendingPreparationWorker<C> {
    #[cfg(test)]
    pub(crate) fn abort_handle_for_test(&self) -> tokio::task::AbortHandle {
        self.worker
            .as_ref()
            .expect("live pending worker")
            .abort_handle_for_test()
    }

    pub(crate) fn new(
        preparation: SubscriptionPreparation<'static, C>,
        operation: PendingBridgeOperation,
        observation: InitialRequestObservation<C>,
        transport: SubscriptionTransport,
        initial: Option<InitialWorkerResources>,
    ) -> Result<Self, Box<PendingPreparationStartFailure<C>>> {
        let (validation, timers) = preparation.into_validation();
        let (activity, reporter) = validation.worker_scope();
        let instrumentation = validation.instrumentation();
        let worker = match ValidationWorker::new(
            &operation,
            async move { validation.prepare_worker() },
            activity.clone(),
            reporter,
            instrumentation,
        ) {
            Ok(worker) => worker,
            Err(_) => {
                let buffer = initial.and_then(InitialWorkerResources::into_finite_buffer);
                let (deadline, buffer) = match timers.into_initial_response_deadline(buffer) {
                    Ok(deadline) => (Some(deadline), None),
                    Err((_, buffer)) => (None, buffer),
                };
                return Err(Box::new(PendingPreparationStartFailure {
                    observation,
                    deadline,
                    buffer,
                }));
            }
        };
        Ok(Self {
            operation: Some(operation),
            observation: Some(observation),
            transport,
            timers: Some(timers),
            worker: Some(worker),
            activity,
            initial,
            terminal_buffer: None,
            cleanup: None,
            terminal: None,
        })
    }

    fn begin_cleanup(&mut self, terminal: PendingTerminal) {
        if self.cleanup.is_some() || self.worker.is_none() {
            return;
        }
        let deadlines = self.timers.take().expect("pending worker timers");
        let deadline_factory = deadlines.factory();
        let Some(operation) = self.operation.take() else {
            self.fail_closed_with_deadlines(deadlines);
            return;
        };
        let terminal_owner = match operation.revoke() {
            Ok(owner) => owner,
            Err(_) => {
                self.fail_closed_with_deadlines(deadlines);
                return;
            }
        };
        let worker = self.worker.take().expect("pending concrete worker");
        worker.abort();
        self.take_terminal_buffer();
        let retained = WorkerRetainedState::pending(deadlines, self.initial.take());
        let cleanup = DetachedCleanup::worker(worker.erase(), retained);
        self.cleanup = Some(terminal_owner.coordinate(cleanup, deadline_factory));
        self.terminal = Some(terminal);
    }

    fn fail_closed(&mut self) {
        self.take_terminal_buffer();
        let retained = self
            .timers
            .take()
            .map_or_else(WorkerRetainedState::empty, |deadlines| {
                WorkerRetainedState::pending(deadlines, self.initial.take())
            });
        if let Some(worker) = self.worker.take() {
            worker.abort();
            worker.erase().retire(RetiredWorkerOwner::orphan(retained));
        } else {
            std::mem::forget(retained);
        }
        if let Some(operation) = self.operation.take() {
            std::mem::forget(operation);
        }
        self.terminal = Some(PendingTerminal::Failed);
    }

    fn fail_closed_with_deadlines(&mut self, deadlines: SubscriptionSetupDeadlines<'static>) {
        self.timers = Some(deadlines);
        self.fail_closed();
    }

    fn finish_terminal(&mut self) -> PendingPreparationOutcome<C> {
        let terminal = self.terminal.take().expect("pending terminal reason");
        let observation = self.observation.take().expect("pending observation");
        let buffer = self.terminal_buffer.take();
        match terminal {
            PendingTerminal::TimedOut => PendingPreparationOutcome::TimedOut {
                observation,
                buffer,
            },
            PendingTerminal::Failed => self.failure_outcome(observation, buffer),
        }
    }

    fn failure_outcome(
        &self,
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    ) -> PendingPreparationOutcome<C> {
        if self.activity.is_managed() && self.activity.task_failed() {
            PendingPreparationOutcome::ObservedFailure {
                observation,
                buffer,
            }
        } else {
            PendingPreparationOutcome::Failed {
                observation,
                buffer,
            }
        }
    }

    fn take_terminal_buffer(&mut self) {
        if self.terminal_buffer.is_none() {
            self.terminal_buffer = self
                .initial
                .as_mut()
                .and_then(InitialWorkerResources::take_response_buffer);
        }
    }
}

impl<C: Send + Sync + 'static> Future for PendingPreparationWorker<C> {
    type Output = PendingPreparationOutcome<C>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(cleanup) = &mut self.cleanup {
            if Pin::new(cleanup).poll(context).is_pending() {
                return Poll::Pending;
            }
            self.cleanup.take();
            return Poll::Ready(self.finish_terminal());
        }
        let deadline_ready = self
            .timers
            .as_mut()
            .expect("pending worker timers")
            .poll_expired(context)
            .is_ready();
        if deadline_ready {
            self.begin_cleanup(PendingTerminal::TimedOut);
            return self.as_mut().poll(context);
        }
        let outcome =
            match Pin::new(self.worker.as_mut().expect("pending concrete worker")).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(outcome) => outcome,
            };
        match outcome {
            ConcreteWorkerOutcome::Completed(Ok(Ok(validated))) => {
                self.worker.take();
                let timers = self.timers.take().expect("pending worker timers");
                Poll::Ready(PendingPreparationOutcome::Prepared(Box::new(
                    PreparedWorkerSubscription {
                        prepared: validated.with_deadlines(timers),
                        operation: self.operation.take().expect("pending operation"),
                        observation: self.observation.take().expect("pending observation"),
                        transport: self.transport,
                        initial: self.initial.take(),
                    },
                )))
            }
            ConcreteWorkerOutcome::Completed(Ok(Err(initialization))) => {
                self.worker.take();
                self.operation.take();
                let buffer = self
                    .initial
                    .take()
                    .and_then(InitialWorkerResources::into_finite_buffer);
                let deadline = match self
                    .timers
                    .take()
                    .expect("pending worker timers")
                    .into_initial_response_deadline(buffer)
                {
                    Ok(deadline) => deadline,
                    Err((_, buffer)) => {
                        return Poll::Ready(PendingPreparationOutcome::Failed {
                            observation: self.observation.take().expect("pending observation"),
                            buffer,
                        });
                    }
                };
                Poll::Ready(PendingPreparationOutcome::Response {
                    deadline: Box::new(deadline),
                    initialization,
                    observation: self.observation.take().expect("pending observation"),
                })
            }
            ConcreteWorkerOutcome::Poisoned => {
                self.begin_cleanup(PendingTerminal::Failed);
                self.as_mut().poll(context)
            }
            ConcreteWorkerOutcome::Completed(Err(
                SubscriptionWorkerRuntimeError::EndpointClosed,
            )) => {
                self.worker.take();
                self.operation.take();
                self.timers.take();
                let buffer = self
                    .initial
                    .take()
                    .and_then(InitialWorkerResources::into_finite_buffer);
                Poll::Ready(PendingPreparationOutcome::Cancelled {
                    observation: self.observation.take().expect("pending observation"),
                    buffer,
                })
            }
            ConcreteWorkerOutcome::Cancelled
            | ConcreteWorkerOutcome::Panicked
            | ConcreteWorkerOutcome::Completed(Err(_)) => {
                self.worker.take();
                self.operation.take();
                self.timers.take();
                let buffer = self
                    .initial
                    .take()
                    .and_then(InitialWorkerResources::into_finite_buffer);
                let observation = self.observation.take().expect("pending observation");
                Poll::Ready(self.failure_outcome(observation, buffer))
            }
        }
    }
}

impl<C: Send + Sync + 'static> Drop for PendingPreparationWorker<C> {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.begin_cleanup(PendingTerminal::Failed);
        }
    }
}
