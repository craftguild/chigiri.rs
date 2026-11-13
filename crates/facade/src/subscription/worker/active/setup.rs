//! Outside acceptance of setup, cancellation and initial response ownership.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{
    ActiveSetupOutcome, ActiveSubscriptionWorker, ActiveWorkerSetup, ActiveWorkerSignal,
    InitialResponseDeadline, SubscriptionWorkerSetup, TerminalReason, WorkerSetupMessage,
};

impl<C: Send + Sync + 'static> Future for ActiveWorkerSetup<C> {
    type Output = ActiveSetupOutcome<C>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(signal) = self.worker_mut().poll_terminal(context) {
            return Poll::Ready(match signal {
                ActiveWorkerSignal::TimedOut => self.take_terminal(true),
                ActiveWorkerSignal::Cancelled => self.take_cancelled(),
                _ => self.take_terminal(false),
            });
        }
        if self.worker_mut().cleanup.is_some() {
            return Poll::Pending;
        }
        if self.worker_mut().activity.setup_cancelled() {
            return match self.worker_mut().poll_join(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => Poll::Ready(self.take_cancelled()),
                Poll::Ready(Err(())) => Poll::Ready(self.take_terminal(false)),
            };
        }
        match self.setup_receiver.poll_take(context) {
            Poll::Ready(Ok(Some(WorkerSetupMessage::Observation(mut observation)))) => {
                let worker = self.worker_mut();
                if worker.observation.is_some() || !observation.accept() {
                    worker.begin_cleanup(TerminalReason::Failed);
                } else {
                    observation.poll(context);
                    worker.observation = Some(observation);
                }
                context.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(Ok(Some(message))) => return Poll::Ready(self.finish_message(message)),
            Poll::Ready(Err(_)) => self.worker_mut().begin_cleanup(TerminalReason::Failed),
            Poll::Ready(Ok(None)) | Poll::Pending => {}
        }
        match self.worker_mut().poll_join(context) {
            Poll::Ready(Ok(())) if self.worker_mut().activity.setup_cancelled() => {
                Poll::Ready(self.take_cancelled())
            }
            Poll::Ready(Ok(())) => Poll::Ready(self.take_terminal(false)),
            Poll::Ready(Err(())) => Poll::Ready(self.take_terminal(false)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<C: Send + Sync + 'static> ActiveWorkerSetup<C> {
    #[cfg(test)]
    pub(crate) fn abort_handle_for_test(&self) -> tokio::task::AbortHandle {
        self.worker
            .as_ref()
            .and_then(|worker| worker.worker.as_ref())
            .expect("live setup worker")
            .abort_handle_for_test()
    }

    fn take_cancelled(&mut self) -> ActiveSetupOutcome<C> {
        if let Some(worker) = &mut self.worker
            && let Some(observation) = &mut worker.observation
        {
            observation.finish(
                chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
                0,
            );
        }
        self.worker.take();
        ActiveSetupOutcome::Cancelled {
            observation: self.observation.take().expect("active setup observation"),
            buffer: self.initial_buffer.take(),
        }
    }

    fn worker_mut(&mut self) -> &mut ActiveSubscriptionWorker {
        self.worker.as_mut().expect("active setup worker")
    }

    fn take_terminal(&mut self, timed_out: bool) -> ActiveSetupOutcome<C> {
        let task_failed = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.activity.task_failed());
        self.worker.take();
        let observation = self.observation.take().expect("active setup observation");
        if timed_out {
            ActiveSetupOutcome::TimedOut {
                observation,
                buffer: self.initial_buffer.take(),
            }
        } else if task_failed {
            ActiveSetupOutcome::ObservedFailure {
                observation,
                buffer: self.initial_buffer.take(),
            }
        } else {
            ActiveSetupOutcome::Failed {
                observation,
                buffer: self.initial_buffer.take(),
            }
        }
    }

    fn finish_message(&mut self, message: WorkerSetupMessage) -> ActiveSetupOutcome<C> {
        if let WorkerSetupMessage::Ready {
            overflow, receipt, ..
        } = &message
        {
            let valid = match (&self.worker_mut().observation, receipt.as_ref()) {
                (Some(owner), receipt) => owner.registered(receipt, *overflow),
                (None, None) => true,
                _ => false,
            };
            if !valid {
                self.worker_mut().begin_cleanup(TerminalReason::Failed);
                return self.take_terminal(false);
            }
        } else if let Some(observation) = &mut self.worker_mut().observation {
            observation.finish(
                chigiri_runtime::__private::SubscriptionTermination::ExecutionFailed,
                0,
            );
        }
        let observation = self.observation.take().expect("active setup observation");
        match message {
            WorkerSetupMessage::Observation(_) => {
                unreachable!("observation transfer is handled first")
            }
            WorkerSetupMessage::Ready {
                headers,
                overflow,
                telemetry,
                ..
            } => {
                self.worker_mut().retained_telemetry = telemetry;
                ActiveSetupOutcome::Ready(SubscriptionWorkerSetup {
                    deadline: InitialResponseDeadline::source(
                        self.worker.take().expect("active setup worker"),
                        self.initial_buffer.take(),
                    ),
                    headers,
                    overflow,
                    observation,
                    transport: self.transport,
                })
            }
            WorkerSetupMessage::Response { response, headers } => ActiveSetupOutcome::Response {
                deadline: InitialResponseDeadline::response(
                    self.worker.take().expect("active setup worker"),
                    self.initial_buffer.take(),
                ),
                response,
                headers,
                observation,
                transport: self.transport,
            },
            WorkerSetupMessage::CompletionResponse { response, outcome } => {
                ActiveSetupOutcome::CompletionResponse {
                    deadline: InitialResponseDeadline::response(
                        self.worker.take().expect("active setup worker"),
                        self.initial_buffer.take(),
                    ),
                    response,
                    outcome,
                    observation,
                }
            }
            WorkerSetupMessage::Rejected(response) => ActiveSetupOutcome::Rejected {
                deadline: InitialResponseDeadline::response(
                    self.worker.take().expect("active setup worker"),
                    self.initial_buffer.take(),
                ),
                response,
                observation,
            },
        }
    }
}
