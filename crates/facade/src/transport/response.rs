//! Converts facade endpoint failures into negotiated GraphQL responses.

use chigiri_runtime::{
    ExecutableSchema, GraphQlError, GraphQlResponse, LifecyclePhase, RequestTelemetry,
};

use chigiri_http::{
    __encode_response_with_status_and_limit, ResponseMediaType, ResponseParts,
    encode_response_with_status,
};

pub(crate) fn error_response(
    media: ResponseMediaType,
    status: u16,
    message: &'static str,
    code: &'static str,
) -> ResponseParts {
    error_value_response(media, status, GraphQlError::new(message).code(code))
}

pub(crate) fn error_value_response(
    media: ResponseMediaType,
    status: u16,
    error: GraphQlError,
) -> ResponseParts {
    let response = GraphQlResponse::request_error(error);
    encode_response_with_status(media, status, &response)
        .unwrap_or_else(|_| ResponseParts::serialization_failure(media))
}

pub(crate) fn timeout_response(media: ResponseMediaType) -> ResponseParts {
    encode_response_with_status(media, 504, &crate::timeout::request_timeout_response())
        .unwrap_or_else(|_| ResponseParts::serialization_failure(media))
}

pub(crate) fn body_timeout_response(media: ResponseMediaType) -> ResponseParts {
    error_response(media, 408, "Request body timed out", "REQUEST_BODY_TIMEOUT")
}

pub(crate) fn body_limit_response(media: ResponseMediaType) -> ResponseParts {
    error_response(media, 413, "Request body is too large", "PAYLOAD_TOO_LARGE")
}

pub(crate) fn internal_error_response<C>(
    schema: &ExecutableSchema<C>,
    media: ResponseMediaType,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    phase: LifecyclePhase,
    component: &'static str,
    telemetry: &RequestTelemetry,
) -> ResponseParts
where
    C: Send + Sync + 'static,
{
    observed_error_response(
        schema,
        media,
        500,
        "Internal server error",
        "INTERNAL_SERVER_ERROR",
        source,
        phase,
        component,
        telemetry,
    )
}

/// It also encapsulates masked errors within the set byte limit when encoding failure occurs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bounded_internal_error_response<C>(
    schema: &ExecutableSchema<C>,
    media: ResponseMediaType,
    maximum_bytes: usize,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    phase: LifecyclePhase,
    component: &'static str,
    telemetry: &RequestTelemetry,
) -> ResponseParts
where
    C: Send + Sync + 'static,
{
    let error_id = schema.observe_internal_error(
        phase,
        "INTERNAL_SERVER_ERROR",
        source,
        Some(telemetry),
        component,
    );
    let mut error = GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR");
    if let Some(error_id) = error_id {
        error = error.extension("errorId", error_id.as_str());
    }
    let response = GraphQlResponse::request_error(error);
    __encode_response_with_status_and_limit(media, 500, &response, maximum_bytes)
        .unwrap_or_else(|_| ResponseParts::serialization_failure(media))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn observed_error_response<C>(
    schema: &ExecutableSchema<C>,
    media: ResponseMediaType,
    status: u16,
    message: &'static str,
    code: &'static str,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    phase: LifecyclePhase,
    component: &'static str,
    telemetry: &RequestTelemetry,
) -> ResponseParts
where
    C: Send + Sync + 'static,
{
    let error = observed_error_value(schema, source, phase, component, telemetry, message, code);
    error_value_response(media, status, error)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn observed_error_value<C>(
    schema: &ExecutableSchema<C>,
    source: &(dyn std::error::Error + Send + Sync + 'static),
    phase: LifecyclePhase,
    component: &'static str,
    telemetry: &RequestTelemetry,
    message: &'static str,
    code: &'static str,
) -> GraphQlError
where
    C: Send + Sync + 'static,
{
    let error_id = schema.observe_internal_error(phase, code, source, Some(telemetry), component);
    let mut error = GraphQlError::new(message).code(code);
    if let Some(error_id) = error_id {
        error = error.extension("errorId", error_id.as_str());
    }
    error
}
