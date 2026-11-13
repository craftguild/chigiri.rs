//! Early opening capture and finite WebSocket HTTP admission ownership.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{
    __preflight_websocket_upgrade, AdmissionDecision, BoxFuture, DeadlineFactory,
    ResponseMediaType, ResponseParts, TransportRequest, WebSocketHttpVersion,
};
use chigiri_runtime::instrumentation::{
    CapturedInstrumentation, InstrumentationKind, instrument_future,
};
use chigiri_runtime::{LifecyclePhase, LifecycleResult, RequestTelemetry};

use super::observation::UpgradeObservation;
use super::{AdmittedWebSocketRequest, AdmittedWebSocketUpgrade, WebSocketUpgradeIngress};
use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::request::MetadataRetentionLimit;
use crate::transport::observation::Phase;
use crate::transport::response::{error_response, internal_error_response, timeout_response};
use crate::transport::{EndpointState, InFlightRequest, TimeoutKind, run};

const UPGRADE_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn handle<'a, 'deadline: 'a, C>(
    state: Arc<EndpointState<C>>,
    request: TransportRequest<'a>,
    version: WebSocketHttpVersion,
    deadlines: &'deadline dyn DeadlineFactory,
) -> BoxFuture<'a, WebSocketUpgradeIngress<'deadline>>
where
    C: Send + Sync + 'static,
{
    let captured = request
        .__instrumentation_capture()
        .cloned()
        .or_else(|| state.schema.capture_instrumentation(&request));
    let handoff = captured
        .as_ref()
        .map(|captured| captured.handoff_scope(InstrumentationKind::Request));
    instrument_future(handoff, async move {
        let in_flight = InFlightRequest::start(Arc::clone(&state.in_flight));
        let telemetry = state.schema.request_telemetry_with_capture(
            request.trace_id().cloned(),
            captured.as_ref(),
            InstrumentationKind::Request,
        );
        let scope = telemetry.instrumentation_scope().cloned();
        let work = handle_owned(
            state, request, version, deadlines, telemetry, captured, in_flight,
        );
        match scope {
            Some(scope) => scope.instrument(work).await,
            None => work.await,
        }
    })
}

async fn handle_owned<'a, 'deadline: 'a, C: Send + Sync + 'static>(
    state: Arc<EndpointState<C>>,
    request: TransportRequest<'a>,
    version: WebSocketHttpVersion,
    deadlines: &'deadline dyn DeadlineFactory,
    telemetry: RequestTelemetry,
    captured: Option<CapturedInstrumentation>,
    in_flight: InFlightRequest,
) -> WebSocketUpgradeIngress<'deadline> {
    let observation = UpgradeObservation::new(state.schema.clone(), telemetry.clone(), in_flight);
    let request = request
        .__with_request_telemetry(&telemetry)
        .__with_deadline_factory(deadlines);
    let mut deadline: BoxFuture<'deadline, ()> = match &captured {
        Some(captured) => {
            let scope = captured.child_handoff_scope(InstrumentationKind::Provider {
                phase: LifecyclePhase::TransportAdmission,
                component: "websocket-upgrade-deadline",
            });
            let timer = scope.run(|| deadlines.sleep(UPGRADE_TIMEOUT));
            Box::pin(scope.instrument(timer))
        }
        None => deadlines.sleep(UPGRADE_TIMEOUT),
    };

    let phase = Phase::start(&state.schema, LifecyclePhase::HttpPreflight, &telemetry);
    let verified = match phase
        .run(|| __preflight_websocket_upgrade(request.method(), version, request.headers()))
    {
        Ok(verified) => {
            phase.finish(LifecycleResult::Succeeded, None);
            verified
        }
        Err(error) => {
            phase.finish(LifecycleResult::Rejected, None);
            return finite(error.into_response_parts(), observation);
        }
    };

    let phase = Phase::start(
        &state.schema,
        LifecyclePhase::TransportAdmission,
        &telemetry,
    );
    match phase
        .instrument(run(
            deadlines,
            state.settings.transport_admission_timeout,
            deadline.as_mut(),
            phase.run(|| state.lifecycle.admit_transport(request)),
        ))
        .await
    {
        Ok(Ok(AdmissionDecision::Continue)) => phase.finish(LifecycleResult::Succeeded, None),
        Ok(Ok(AdmissionDecision::Reject(rejection))) => {
            phase.finish(LifecycleResult::Rejected, None);
            return finite(
                rejection.response_parts(ResponseMediaType::GraphQlResponseJson),
                observation,
            );
        }
        Ok(Err(error)) => {
            phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
            let response = internal_error_response(
                &state.schema,
                ResponseMediaType::GraphQlResponseJson,
                &error,
                LifecyclePhase::TransportAdmission,
                "websocket-transport-admission",
                &telemetry,
            );
            return finite(response, observation);
        }
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
            phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            return finite(
                timeout_response(ResponseMediaType::GraphQlResponseJson),
                observation,
            );
        }
    }

    let limit = MetadataRetentionLimit::from_validated(&state.subscriptions.settings);
    let retained = match AdmittedWebSocketRequest::capture(
        request,
        &telemetry,
        verified.key(),
        limit,
        captured.as_ref(),
    ) {
        Ok(retained) => retained,
        Err(_) => {
            let response = error_response(
                ResponseMediaType::GraphQlResponseJson,
                413,
                "WebSocket request metadata exceeds its limit",
                "BAD_USER_INPUT",
            );
            observation.finish(LifecycleResult::Rejected, Some("BAD_USER_INPUT"));
            return WebSocketUpgradeIngress::Finite(response);
        }
    };
    let resources = &state.subscriptions.resources;
    let owner = resources.delivery().byte_owner();
    let connection = match resources.open_connection() {
        Ok(connection) => connection,
        Err(error) => {
            return resource_failure(&state, error, observation);
        }
    };
    observation.finish(LifecycleResult::Succeeded, None);
    WebSocketUpgradeIngress::Admitted(AdmittedWebSocketUpgrade {
        request: Box::new(retained),
        connection,
        owner,
        deadline,
    })
}

fn finite<'deadline, C: Send + Sync + 'static>(
    response: ResponseParts,
    observation: UpgradeObservation<C>,
) -> WebSocketUpgradeIngress<'deadline> {
    observation.finish_status(response.status);
    WebSocketUpgradeIngress::Finite(response)
}

fn resource_failure<'deadline, C: Send + Sync + 'static>(
    state: &EndpointState<C>,
    error: BridgeResourceError,
    observation: UpgradeObservation<C>,
) -> WebSocketUpgradeIngress<'deadline> {
    if error == BridgeResourceError::Capacity {
        let response = error_response(
            ResponseMediaType::GraphQlResponseJson,
            503,
            "WebSocket connection capacity is unavailable",
            "SUBSCRIPTION_LIMIT_EXCEEDED",
        );
        observation.finish(
            LifecycleResult::Rejected,
            Some("SUBSCRIPTION_LIMIT_EXCEEDED"),
        );
        return WebSocketUpgradeIngress::Finite(response);
    }
    let response = internal_error_response(
        &state.schema,
        ResponseMediaType::GraphQlResponseJson,
        &error,
        LifecyclePhase::TransportAdmission,
        "websocket-connection-admission",
        observation.telemetry(),
    );
    observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
    WebSocketUpgradeIngress::Finite(response)
}
