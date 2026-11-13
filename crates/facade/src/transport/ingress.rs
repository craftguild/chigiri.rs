//! One admitted, bounded decode/source/hook path before execution dispatch.

mod negotiation;

use std::future::Future;
use std::pin::Pin;

use chigiri_http::{
    AdmissionDecision, DeadlineFactory, DecodedRequestView, RequestBody, ResponseParts,
    TransportRequest,
};
use chigiri_runtime::{GraphQlRequest, LifecyclePhase, LifecycleResult, RequestTelemetry};

pub(crate) use negotiation::{HttpIngressMode, HttpNegotiation};

use super::deadline::HttpDeadlineMonitor;
use super::observation::Phase;
use super::response::{
    body_limit_response, body_timeout_response, internal_error_response, timeout_response,
};
use super::{CollectError, EndpointState, TimeoutKind, collect};

pub(crate) struct ReceivedHttpRequest {
    pub(crate) graph_request: GraphQlRequest,
    pub(crate) raw_body: Vec<u8>,
    pub(crate) negotiation: HttpNegotiation,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn receive<C, B>(
    state: &EndpointState<C>,
    request: TransportRequest<'_>,
    body: &mut B,
    deadlines: &dyn DeadlineFactory,
    mut overall: Pin<&mut (dyn Future<Output = ()> + Send)>,
    telemetry: &RequestTelemetry,
    mode: HttpIngressMode,
    monitor: &HttpDeadlineMonitor,
) -> Result<ReceivedHttpRequest, ResponseParts>
where
    C: Send + Sync + 'static,
    B: RequestBody,
{
    let phase = Phase::start(&state.schema, LifecyclePhase::HttpPreflight, telemetry);
    let negotiation = match phase.run(|| HttpNegotiation::preflight(mode, request)) {
        Ok(value) => {
            phase.finish(LifecycleResult::Succeeded, None);
            value
        }
        Err(error) => {
            phase.finish(LifecycleResult::Rejected, None);
            return Err(error.into_response_parts());
        }
    };
    let media = negotiation.provisional_media();
    let request = request.__with_response_media(media);
    let phase = Phase::start(&state.schema, LifecyclePhase::TransportAdmission, telemetry);
    match phase
        .monitor(
            monitor,
            deadlines,
            state.settings.transport_admission_timeout,
            overall.as_mut(),
            || state.lifecycle.admit_transport(request),
        )
        .await
    {
        Ok(Ok(AdmissionDecision::Continue)) => phase.finish(LifecycleResult::Succeeded, None),
        Ok(Ok(AdmissionDecision::Reject(rejection))) => {
            phase.finish(LifecycleResult::Rejected, None);
            return Err(rejection.response_parts(media));
        }
        Ok(Err(error)) => {
            phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
            return Err(internal_error_response(
                &state.schema,
                media,
                &error,
                LifecyclePhase::TransportAdmission,
                "transport-admission",
                telemetry,
            ));
        }
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
            phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            return Err(timeout_response(media));
        }
    }
    let phase = Phase::start(&state.schema, LifecyclePhase::BodyRead, telemetry);
    let bytes = if negotiation.body_required() {
        if state.settings.body_limit == 0 {
            phase.finish(LifecycleResult::Rejected, Some("PAYLOAD_TOO_LARGE"));
            return Err(body_limit_response(media));
        }
        match phase
            .monitor(
                monitor,
                deadlines,
                state.settings.body_read_timeout,
                overall.as_mut(),
                || collect(body, state.settings.body_limit),
            )
            .await
        {
            Ok(Ok(bytes)) => {
                phase.finish(LifecycleResult::Succeeded, None);
                bytes
            }
            Ok(Err(CollectError::TooLarge)) => {
                phase.finish(LifecycleResult::Rejected, Some("PAYLOAD_TOO_LARGE"));
                return Err(body_limit_response(media));
            }
            Ok(Err(CollectError::Read(error))) => {
                phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
                return Err(internal_error_response(
                    &state.schema,
                    media,
                    &error,
                    LifecyclePhase::BodyRead,
                    "request-body",
                    telemetry,
                ));
            }
            Err(TimeoutKind::Phase) => {
                phase.finish(LifecycleResult::TimedOut, Some("REQUEST_BODY_TIMEOUT"));
                return Err(body_timeout_response(media));
            }
            Err(TimeoutKind::Overall) => {
                phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
                return Err(timeout_response(media));
            }
        }
    } else {
        phase.finish(LifecycleResult::Succeeded, None);
        Vec::new()
    };
    let phase = Phase::start(&state.schema, LifecyclePhase::HttpDecode, telemetry);
    let decoded = match phase.run(|| negotiation.decode(request, &bytes)) {
        Ok(decoded) => {
            phase.finish(LifecycleResult::Succeeded, None);
            decoded
        }
        Err(error) => {
            phase.finish(LifecycleResult::Rejected, None);
            return Err(error.into_response_parts());
        }
    };
    let graph_request = super::source::resolve(
        state,
        deadlines,
        overall.as_mut(),
        decoded.envelope.with_request_telemetry(telemetry.clone()),
        media,
        telemetry,
        monitor,
    )
    .await?;
    let decoded_view = DecodedRequestView::new(&graph_request, negotiation.method(), media);
    let phase = Phase::start(&state.schema, LifecyclePhase::DecodedRequestHook, telemetry);
    match phase
        .monitor(
            monitor,
            deadlines,
            state.settings.decoded_request_timeout,
            overall.as_mut(),
            || state.lifecycle.inspect_decoded(decoded_view),
        )
        .await
    {
        Ok(Ok(AdmissionDecision::Continue)) => phase.finish(LifecycleResult::Succeeded, None),
        Ok(Ok(AdmissionDecision::Reject(rejection))) => {
            phase.finish(LifecycleResult::Rejected, None);
            return Err(rejection.response_parts(media));
        }
        Ok(Err(error)) => {
            phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
            return Err(internal_error_response(
                &state.schema,
                media,
                &error,
                LifecyclePhase::DecodedRequestHook,
                "decoded-request-hook",
                telemetry,
            ));
        }
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
            phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            return Err(timeout_response(media));
        }
    }
    Ok(ReceivedHttpRequest {
        graph_request,
        raw_body: bytes,
        negotiation,
    })
}
