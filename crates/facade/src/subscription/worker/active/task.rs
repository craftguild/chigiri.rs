//! Resource-free source task and its fixed-capacity supervisor ports.

use chigiri_http::ResponseCompletionOutcome;

use super::initial_command::{InitialResponseCommand, InitialResponseTaskPort};
use super::initial_owner::SourceAndInitialCompletion;
use super::output::WorkerSetupMessage;
use crate::subscription::control::InitialCompletionFallbacks;
use crate::subscription::driver::input::ValidatedEndpointSubscription;
use crate::subscription::driver::{
    EndpointSubscription, SubscriptionInitialization, SubscriptionOutput, initialize_monitored,
};
use crate::subscription::worker::port::{PortReceiver, PortSender};
use crate::subscription::worker::{DeadlineArmer, SubscriptionWorkerRuntimeError};

pub(super) struct EventPorts {
    pub(super) demand: PortReceiver<()>,
    pub(super) output: PortSender<SubscriptionOutput>,
    pub(super) activity: crate::managed::EventActivity,
}

pub(super) async fn run<C: Send + Sync + 'static>(
    validated: ValidatedEndpointSubscription<C>,
    armer: DeadlineArmer,
    setup_sender: PortSender<WorkerSetupMessage>,
    mut initial_response: InitialResponseTaskPort,
    fallbacks: Option<InitialCompletionFallbacks>,
    events: EventPorts,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    let EventPorts {
        demand: mut demand_receiver,
        output: event_sender,
        activity,
    } = events;
    let mut owner = SourceAndInitialCompletion::initial(
        fallbacks,
        activity.is_managed().then(|| activity.clone()),
    );
    let execution = validated.execution();
    let initialized = match execution
        .pending_phase(initialize_monitored(
            validated,
            &armer,
            &mut owner,
            &setup_sender,
        ))
        .await
        .unwrap_or_else(|| Err(execution.setup_stopped_error()))
    {
        Ok(initialized) => initialized,
        Err(SubscriptionWorkerRuntimeError::EndpointClosed) => {
            return finish_cancelled(&mut owner, &mut initial_response, &activity);
        }
        Err(error) => {
            let outcome = initial_response.failed_initialization_outcome();
            let success = owner.finish_terminal(outcome);
            initial_response.send_terminal_result(success);
            return Err(error);
        }
    };
    let stopped = !execution.authorize_start();
    let subscription = match initialized {
        SubscriptionInitialization::Ready {
            subscription,
            headers,
            completion,
        } => {
            drop(completion);
            owner.active_source(subscription);
            if stopped {
                return finish_cancelled(&mut owner, &mut initial_response, &activity);
            }
            let overflow = owner
                .overflow()
                .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
            let receipt = owner.observation_receipt();
            let telemetry = owner.retained_telemetry().cloned();
            #[cfg(test)]
            crate::subscription::failure::assert_shared_ids(
                owner.retained_telemetry(),
                telemetry.as_ref(),
            );
            let _ = setup_sender.try_send(WorkerSetupMessage::Ready {
                headers,
                overflow,
                receipt,
                telemetry,
            });
            drop(setup_sender);
            let Some(subscription) =
                settle_ready(&mut owner, &mut initial_response, &execution, &activity).await?
            else {
                return Ok(());
            };
            subscription
        }
        SubscriptionInitialization::Response {
            response,
            headers,
            completion,
        } => {
            drop(completion);
            if stopped {
                return finish_cancelled(&mut owner, &mut initial_response, &activity);
            }
            let _ = setup_sender.try_send(WorkerSetupMessage::Response { response, headers });
            drop(setup_sender);
            settle_response(&mut owner, &mut initial_response, &execution, &activity).await?;
            return Ok(());
        }
        SubscriptionInitialization::CompletionResponse {
            response,
            completion,
            outcome,
        } => {
            drop(completion);
            if stopped {
                return finish_cancelled(&mut owner, &mut initial_response, &activity);
            }
            let _ =
                setup_sender.try_send(WorkerSetupMessage::CompletionResponse { response, outcome });
            drop(setup_sender);
            settle_response(&mut owner, &mut initial_response, &execution, &activity).await?;
            return Ok(());
        }
        SubscriptionInitialization::Rejected(response) => {
            if stopped {
                return finish_cancelled(&mut owner, &mut initial_response, &activity);
            }
            setup_sender
                .try_send(WorkerSetupMessage::Rejected(response))
                .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
            return Ok(());
        }
    };
    run_events(
        *subscription,
        &armer,
        &mut demand_receiver,
        &event_sender,
        &activity,
    )
    .await
}

async fn settle_ready<C: Send + Sync + 'static>(
    owner: &mut SourceAndInitialCompletion<C>,
    port: &mut InitialResponseTaskPort,
    execution: &crate::subscription::execution_state::SubscriptionExecutionState<C>,
    activity: &crate::managed::EventActivity,
) -> Result<Option<Box<EndpointSubscription<C>>>, SubscriptionWorkerRuntimeError> {
    let Some(command) = execution.pending_phase(port.receive()).await else {
        finish_cancelled(owner, port, activity)?;
        return Ok(None);
    };
    let command = command?;
    match command {
        Some(InitialResponseCommand::Complete { response, outcome }) => {
            let response = match owner.finish_normal(response, outcome) {
                Ok(response) => response,
                Err(()) => {
                    port.send_failure();
                    return Ok(None);
                }
            };
            if let Err(response) = port.send_result(response) {
                drop(response);
                return Ok(None);
            }
            Ok(owner.take_source())
        }
        Some(InitialResponseCommand::Terminate(outcome)) => {
            let success = owner.finish_terminal(outcome);
            port.send_terminal_result(success);
            if !success {
                return Err(SubscriptionWorkerRuntimeError::Invariant);
            }
            Ok(None)
        }
        None => {
            let _ = owner.finish_terminal(ResponseCompletionOutcome::Failed);
            Ok(None)
        }
    }
}

async fn settle_response<C: Send + Sync + 'static>(
    owner: &mut SourceAndInitialCompletion<C>,
    port: &mut InitialResponseTaskPort,
    execution: &crate::subscription::execution_state::SubscriptionExecutionState<C>,
    activity: &crate::managed::EventActivity,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    let Some(command) = execution.pending_phase(port.receive()).await else {
        return finish_cancelled(owner, port, activity);
    };
    match command? {
        Some(InitialResponseCommand::Complete { response, outcome }) => {
            let response = match owner.finish_normal(response, outcome) {
                Ok(response) => response,
                Err(()) => {
                    port.send_failure();
                    return Ok(());
                }
            };
            if let Err(response) = port.send_result(response) {
                drop(response);
            }
        }
        Some(InitialResponseCommand::Terminate(outcome)) => {
            let success = owner.finish_terminal(outcome);
            port.send_terminal_result(success);
            if !success {
                return Err(SubscriptionWorkerRuntimeError::Invariant);
            }
        }
        None => {
            let _ = owner.finish_terminal(ResponseCompletionOutcome::Failed);
        }
    }
    Ok(())
}

fn finish_cancelled<C: Send + Sync + 'static>(
    owner: &mut SourceAndInitialCompletion<C>,
    port: &mut InitialResponseTaskPort,
    activity: &crate::managed::EventActivity,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    let outcome = activity.control().and_then(|control| control.outcome());
    let selected = port.failed_initialization_outcome();
    let success = owner.finish_terminal(
        if outcome == Some(crate::ShutdownOutcome::Expired)
            || selected == ResponseCompletionOutcome::TimedOut
        {
            ResponseCompletionOutcome::TimedOut
        } else {
            ResponseCompletionOutcome::Failed
        },
    );
    port.send_terminal_result(success);
    if success && outcome != Some(crate::ShutdownOutcome::InfrastructureFailure) {
        activity.cancel_setup();
        Ok(())
    } else {
        Err(SubscriptionWorkerRuntimeError::Invariant)
    }
}

pub(crate) async fn run_events<C: Send + Sync + 'static>(
    subscription: EndpointSubscription<C>,
    armer: &DeadlineArmer,
    demand_receiver: &mut PortReceiver<()>,
    event_sender: &PortSender<SubscriptionOutput>,
    activity: &crate::managed::EventActivity,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    use crate::subscription::worker::task_ownership::{TaskOwnedFuture, TaskOwnedValue};
    let mut subscription = TaskOwnedValue::new(subscription, Some(activity.clone()));
    while demand_receiver.receive().await?.is_some() {
        let output = {
            let mut next = TaskOwnedFuture::new(
                subscription.next_monitored(armer, activity),
                Some(activity.clone()),
            );
            std::future::poll_fn(|context| {
                if let std::task::Poll::Ready(result) = demand_receiver.poll_closed(context) {
                    return std::task::Poll::Ready(result.map(|()| None));
                }
                std::pin::Pin::new(&mut next)
                    .poll(context)
                    .map(|result| result.map(Some))
            })
            .await?
        };
        let Some(output) = output else {
            return Ok(());
        };
        let terminal = !matches!(output, SubscriptionOutput::Event(_));
        event_sender
            .try_send(output)
            .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
        if terminal {
            if subscription.host_observation_receipt().is_some() {
                // The outside owner freezes delivery and logs the logical
                // decision before closing this existing demand port. Retain
                // the source here until then, without another queue or task.
                std::future::poll_fn(|context| demand_receiver.poll_closed(context)).await?;
            }
            return Ok(());
        }
    }
    Ok(())
}
