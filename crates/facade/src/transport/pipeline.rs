//! Runs the facade's finite compatibility pipeline over shared admitted ingress.

use std::sync::Arc;

use chigiri_runtime::RequestTelemetry;

use super::EndpointState;
use super::ingress::{HttpIngressMode, receive};
use super::observation::{Phase, status_result};
use super::response::timeout_response;
use chigiri_http::{
    DeadlineFactory, RequestBody, ResponseMediaType, ResponseParts, TransportRequest,
    negotiate_response_media,
};

pub(crate) async fn handle<C, B>(
    state: Arc<EndpointState<C>>,
    request: TransportRequest<'_>,
    mut body: B,
    deadlines: &dyn DeadlineFactory,
    telemetry: RequestTelemetry,
) -> ResponseParts
where
    C: Send + Sync + 'static,
    B: RequestBody,
{
    let request_phase = Phase::request(&state.schema, &telemetry);
    let request = request
        .__with_request_telemetry(&telemetry)
        .__with_deadline_factory(deadlines);
    let response = handle_inner(&state, request, &mut body, deadlines, &telemetry).await;
    let (result, code) = status_result(response.status);
    request_phase.finish(result, code);
    response
}

async fn handle_inner<C, B>(
    state: &EndpointState<C>,
    request: TransportRequest<'_>,
    body: &mut B,
    deadlines: &dyn DeadlineFactory,
    telemetry: &RequestTelemetry,
) -> ResponseParts
where
    C: Send + Sync + 'static,
    B: RequestBody,
{
    let initial_media = negotiate_response_media(request.headers())
        .unwrap_or(ResponseMediaType::GraphQlResponseJson);
    if state.settings.request_timeout.is_zero() {
        return timeout_response(initial_media);
    }
    let mut overall = deadlines.sleep(state.settings.request_timeout);
    let received = match receive(
        state,
        request,
        body,
        deadlines,
        overall.as_mut(),
        telemetry,
        HttpIngressMode::Finite,
        &super::deadline::HttpDeadlineMonitor::default(),
    )
    .await
    {
        Ok(received) => received,
        Err(response) => return response,
    };
    let media = received.negotiation.provisional_media();
    super::execution::execute(
        state,
        deadlines,
        overall.as_mut(),
        request.__with_response_media(media),
        received.graph_request,
        received.negotiation.method(),
        media,
        &received.raw_body,
        &super::deadline::HttpDeadlineMonitor::default(),
    )
    .await
}
