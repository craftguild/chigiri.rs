//! One-shot initial-response completion inside the reserved source worker.

use std::task::{Context, Poll};

use chigiri_http::{ResponseCompletionOutcome, ResponseParts};

use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::port::{PortReceiver, PortSender, channel};

pub(super) enum InitialResponseCommand {
    Complete {
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    },
    Terminate(ResponseCompletionOutcome),
}

pub(super) struct InitialResponseSupervisor {
    sender: PortSender<InitialResponseCommand>,
    result: PortReceiver<InitialResponseTaskResult>,
    state: InitialResponseState,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InitialResponseState {
    Awaiting,
    Completing,
    Terminal,
    Abandoned,
    Completed,
}

pub(super) struct InitialResponseTaskPort {
    command: PortReceiver<InitialResponseCommand>,
    result: PortSender<InitialResponseTaskResult>,
}

enum InitialResponseTaskResult {
    Response(ResponseParts),
    Terminated,
    Failed,
}

pub(super) fn initial_response_channel() -> (InitialResponseSupervisor, InitialResponseTaskPort) {
    let (command_sender, command_receiver) = channel();
    let (result_sender, result_receiver) = channel();
    (
        InitialResponseSupervisor {
            sender: command_sender,
            result: result_receiver,
            state: InitialResponseState::Awaiting,
        },
        InitialResponseTaskPort {
            command: command_receiver,
            result: result_sender,
        },
    )
}

impl InitialResponseSupervisor {
    pub(super) fn complete(
        &mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        if self.state != InitialResponseState::Awaiting {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        self.sender
            .try_send(InitialResponseCommand::Complete { response, outcome })
            .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
        self.state = InitialResponseState::Completing;
        Ok(())
    }

    pub(super) fn terminate(&mut self, outcome: ResponseCompletionOutcome) {
        if self.state == InitialResponseState::Awaiting {
            let _ = self
                .sender
                .try_send(InitialResponseCommand::Terminate(outcome));
            self.state = InitialResponseState::Terminal;
        }
        self.sender.close();
        if self.state != InitialResponseState::Terminal {
            self.result.close();
        }
    }

    pub(super) fn abandon(&mut self) {
        if self.state == InitialResponseState::Awaiting {
            self.state = InitialResponseState::Abandoned;
        }
        self.sender.close();
        self.result.close();
    }

    pub(super) fn is_awaiting(&self) -> bool {
        self.state == InitialResponseState::Awaiting
    }

    /// JoinReady makes the terminal acknowledgment final without another wait.
    pub(super) fn terminal_failed(&mut self, context: &mut Context<'_>) -> bool {
        if self.state != InitialResponseState::Terminal {
            return false;
        }
        match self.result.poll_take(context) {
            Poll::Ready(Ok(Some(InitialResponseTaskResult::Terminated))) => {
                self.state = InitialResponseState::Completed;
                false
            }
            _ => true,
        }
    }

    pub(super) fn poll_result(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<ResponseParts, SubscriptionWorkerRuntimeError>> {
        match self.result.poll_take(context) {
            Poll::Ready(Ok(Some(InitialResponseTaskResult::Response(response)))) => {
                self.state = InitialResponseState::Completed;
                Poll::Ready(Ok(response))
            }
            Poll::Ready(Ok(Some(InitialResponseTaskResult::Failed))) => {
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Shutdown))
            }
            Poll::Ready(Ok(Some(InitialResponseTaskResult::Terminated))) => {
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
            }
            Poll::Ready(Ok(None)) => Poll::Ready(Err(SubscriptionWorkerRuntimeError::Shutdown)),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl InitialResponseTaskPort {
    /// An initializer failure must honor an already selected terminal command.
    pub(super) fn failed_initialization_outcome(&mut self) -> ResponseCompletionOutcome {
        match self.command.try_take() {
            Ok(Some(InitialResponseCommand::Terminate(outcome))) => outcome,
            _ => ResponseCompletionOutcome::Failed,
        }
    }

    pub(super) async fn receive(
        &mut self,
    ) -> Result<Option<InitialResponseCommand>, SubscriptionWorkerRuntimeError> {
        self.command.receive().await
    }

    pub(super) fn send_result(&self, response: ResponseParts) -> Result<(), ResponseParts> {
        self.result
            .try_send(InitialResponseTaskResult::Response(response))
            .map_err(|result| match result {
                InitialResponseTaskResult::Response(response) => response,
                InitialResponseTaskResult::Failed | InitialResponseTaskResult::Terminated => {
                    unreachable!("response result changed kind")
                }
            })
    }

    pub(super) fn send_failure(&self) {
        let _ = self.result.try_send(InitialResponseTaskResult::Failed);
    }

    pub(super) fn send_terminal_result(&self, success: bool) {
        let result = if success {
            InitialResponseTaskResult::Terminated
        } else {
            InitialResponseTaskResult::Failed
        };
        let _ = self.result.try_send(result);
    }
}
