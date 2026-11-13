//! Resource-free WebSocket source setup and sequential event loop.

use chigiri_http::{RequestPhase, SubscriptionTransport};
use chigiri_runtime::__private::{PreparedWebSocketOperation, SubscriptionStart};
use chigiri_runtime::{GraphQlError, GraphQlResponse, LifecyclePhase, RequestTelemetry};

use super::WebSocketSetupMessage;
use crate::subscription::driver::input::ValidatedWebSocketOperation;
use crate::subscription::driver::{
    EndpointSubscription, MonitoredSourceStart, SubscriptionOutput, start_source_monitored,
};
use crate::subscription::preparation::{ContextPreparation, prepare_context_monitored};
use crate::subscription::worker::port::{PortReceiver, PortSender};
use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, SubscriptionWorkerRuntimeError, guard_instrumented_work,
    run_events,
};

pub(super) async fn run<C: Send + Sync + 'static>(
    validated: ValidatedWebSocketOperation<C>,
    armer: DeadlineArmer,
    setup_sender: PortSender<WebSocketSetupMessage>,
    mut demand_receiver: PortReceiver<()>,
    event_sender: PortSender<SubscriptionOutput>,
    activity: crate::managed::EventActivity,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    let telemetry = match &validated.prepared {
        PreparedWebSocketOperation::Subscription(prepared) => prepared.setup_telemetry().cloned(),
        PreparedWebSocketOperation::OneShot(_) => {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
    };
    let instrumentation = telemetry
        .as_ref()
        .and_then(RequestTelemetry::instrumentation_scope)
        .cloned();
    let work = prepare(
        validated,
        armer.clone(),
        &setup_sender,
        activity.clone(),
        telemetry,
    );
    let result =
        chigiri_runtime::instrumentation::instrument_future(instrumentation.clone(), async {
            if activity.is_managed() {
                guard_instrumented_work(work, &activity).await
            } else {
                work.await
            }
        })
        .await;
    // Failure must be visible before port closure wakes the outer setup receiver.
    // Keep both senders here, outside the guarded application future's unwind.
    setup_sender.close();
    if let Some(scope) = instrumentation {
        let outcome = if activity.setup_cancelled() {
            chigiri_runtime::LifecycleResult::Cancelled
        } else {
            match &result {
                Ok(Some(_)) => chigiri_runtime::LifecycleResult::Succeeded,
                Ok(None) => chigiri_runtime::LifecycleResult::Rejected,
                Err(_) => chigiri_runtime::LifecycleResult::Failed,
            }
        };
        scope.finish(outcome);
    }
    let Some(subscription) = result? else {
        return Ok(());
    };
    let work = run_events(
        subscription,
        &armer,
        &mut demand_receiver,
        &event_sender,
        &activity,
    );
    if activity.is_managed() {
        guard_instrumented_work(work, &activity).await
    } else {
        work.await
    }
}

async fn prepare<C: Send + Sync + 'static>(
    validated: ValidatedWebSocketOperation<C>,
    armer: DeadlineArmer,
    setup_sender: &PortSender<WebSocketSetupMessage>,
    activity: crate::managed::EventActivity,
    telemetry: Option<RequestTelemetry>,
) -> Result<Option<EndpointSubscription<C>>, SubscriptionWorkerRuntimeError> {
    let ValidatedWebSocketOperation {
        state,
        prepared,
        mut request,
    } = validated;
    let mut prepared = match prepared {
        PreparedWebSocketOperation::Subscription(prepared) => *prepared,
        PreparedWebSocketOperation::OneShot(_) => {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
    };
    let mut setup = armer.setup()?;
    let mut context_deadline = armer.arm(DeadlineRole::Phase, state.settings.context_timeout)?;
    if !state.authorize_start() {
        activity.cancel_setup();
        return Ok(None);
    }
    let fresh = state
        .pending_phase(prepare_context_monitored(
            &state.schema,
            &state.context_factory,
            &request,
            RequestPhase::SubscriptionSetup {
                transport: SubscriptionTransport::WebSocket,
            },
            armer.factory(),
            &mut context_deadline,
            &mut setup,
            telemetry.as_ref(),
        ))
        .await;
    let Some(fresh) = fresh else {
        activity.cancel_setup();
        return Ok(None);
    };
    let fresh = fresh?;
    if !matches!(fresh, ContextPreparation::TimedOut { .. }) {
        context_deadline.clear()?;
    }
    let fresh = match fresh {
        ContextPreparation::Ready(fresh) => fresh,
        ContextPreparation::Rejected { rejection, .. } => {
            setup.clear()?;
            return send_response(
                setup_sender,
                GraphQlResponse::request_error(rejection.error().clone()),
            );
        }
        ContextPreparation::Internal { error, telemetry } => {
            setup.clear()?;
            return send_response(
                setup_sender,
                internal_failure(&state.schema, &error, &telemetry),
            );
        }
        ContextPreparation::TimedOut { .. } => {
            return send_response(setup_sender, crate::timeout::request_timeout_response());
        }
    };
    if !state.authorize_start() {
        activity.cancel_setup();
        return Ok(None);
    }
    let (context, context_tail) = fresh.into_websocket_execution();
    if let Err(response) = crate::subscription::worker::observation::transfer(
        &mut prepared,
        &context,
        &mut request,
        state.subscriptions.limits().max_retained_metadata_bytes,
        setup_sender,
    ) {
        setup.clear()?;
        return send_response(setup_sender, *response);
    }
    if !state.authorize_start() {
        activity.cancel_setup();
        return Ok(None);
    }
    let started = state
        .pending_phase(start_source_monitored(
            prepared,
            &context,
            state.settings.execution_timeout,
            &armer,
            &mut setup,
        ))
        .await;
    let Some(started) = started else {
        activity.cancel_setup();
        return Ok(None);
    };
    let started = started?;
    if !state.authorize_start() {
        activity.cancel_setup();
        return Ok(None);
    }
    drop(context);
    drop(context_tail);
    let (started, source_deadline) = match started {
        MonitoredSourceStart::Completed { started, deadline } => (started, deadline),
        MonitoredSourceStart::TimedOut => {
            return send_response(setup_sender, crate::timeout::request_timeout_response());
        }
    };
    let session = match started {
        SubscriptionStart::Ready(session) => session,
        SubscriptionStart::Response(response) => {
            source_deadline.clear()?;
            setup.clear()?;
            return send_response(setup_sender, response);
        }
    };
    let subscription = EndpointSubscription::from_websocket_source(state, request, *session);
    let overflow = subscription
        .overflow()
        .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
    source_deadline.clear()?;
    setup.clear()?;
    let telemetry = subscription.retained_telemetry().cloned();
    #[cfg(test)]
    crate::subscription::failure::assert_shared_ids(
        subscription.retained_telemetry(),
        telemetry.as_ref(),
    );
    setup_sender
        .try_send(WebSocketSetupMessage::Ready {
            overflow,
            receipt: subscription.host_observation_receipt().cloned(),
            telemetry,
        })
        .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
    setup_sender.close();
    Ok(Some(subscription))
}

fn send_response<C: Send + Sync + 'static>(
    sender: &PortSender<WebSocketSetupMessage>,
    response: GraphQlResponse,
) -> Result<Option<EndpointSubscription<C>>, SubscriptionWorkerRuntimeError> {
    sender
        .try_send(WebSocketSetupMessage::Response(response))
        .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
    sender.close();
    Ok(None)
}

fn internal_failure<C: Send + Sync + 'static>(
    schema: &chigiri_runtime::ExecutableSchema<C>,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    telemetry: &RequestTelemetry,
) -> GraphQlResponse {
    let id = schema.observe_internal_error(
        LifecyclePhase::ContextConstruction,
        "INTERNAL_SERVER_ERROR",
        source,
        Some(telemetry),
        "websocket-subscription-context",
    );
    let mut error = GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR");
    if let Some(id) = id {
        error = error.extension("errorId", id.as_str());
    }
    GraphQlResponse::request_error(error)
}
