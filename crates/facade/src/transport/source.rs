//! Maps facade operation-source results to transport-neutral responses.

use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "subscription-worker")]
use chigiri_runtime::GraphQlResponse;
use chigiri_runtime::{
    ExecutableSchema, GraphQlError, LifecyclePhase, LifecycleResult, RequestTelemetry,
};

use super::deadline::HttpDeadlineMonitor;
use super::observation::Phase;
use super::response::{
    error_value_response, observed_error_response, observed_error_value, timeout_response,
};
use super::{EndpointState, TimeoutKind};
use chigiri_http::{
    DeadlineFactory, RequestEnvelope, ResponseMediaType, ResponseParts, SourceResolutionError,
};

pub(crate) async fn resolve<C>(
    state: &EndpointState<C>,
    deadlines: &dyn DeadlineFactory,
    overall: Pin<&mut (dyn Future<Output = ()> + Send)>,
    envelope: RequestEnvelope,
    media: ResponseMediaType,
    telemetry: &RequestTelemetry,
    monitor: &HttpDeadlineMonitor,
) -> Result<chigiri_runtime::GraphQlRequest, ResponseParts>
where
    C: Send + Sync + 'static,
{
    let phase = Phase::start(
        &state.schema,
        LifecyclePhase::OperationSourceResolution,
        telemetry,
    );
    match phase
        .monitor(
            monitor,
            deadlines,
            state.settings.source_resolution_timeout,
            overall,
            || state.lifecycle.resolve_source(envelope),
        )
        .await
    {
        Ok(Ok(request)) => {
            phase.finish(LifecycleResult::Succeeded, None);
            Ok(request)
        }
        Ok(Err(error)) => {
            let failure = ObservedSourceFailure::new(&state.schema, error, telemetry);
            let (result, code) = failure.lifecycle();
            phase.finish(result, code);
            Err(failure.into_http_response(media))
        }
        Err(TimeoutKind::Phase) => {
            phase.finish(
                LifecycleResult::TimedOut,
                Some("PERSISTED_OPERATION_UNAVAILABLE"),
            );
            Err(observed_error_response(
                &state.schema,
                media,
                503,
                "Persisted operation service is unavailable",
                "PERSISTED_OPERATION_UNAVAILABLE",
                &OperationSourceTimeout,
                LifecyclePhase::OperationSourceResolution,
                "operation-source",
                telemetry,
            ))
        }
        Err(TimeoutKind::Overall) => {
            phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            Err(timeout_response(media))
        }
    }
}

pub(crate) struct ObservedSourceFailure {
    result: LifecycleResult,
    code: Option<&'static str>,
    response: SourceFailureResponse,
}

enum SourceFailureResponse {
    Rejection(Box<chigiri_http::Rejection>),
    GraphQl { status: u16, error: GraphQlError },
}

impl ObservedSourceFailure {
    pub(crate) fn new<C>(
        schema: &ExecutableSchema<C>,
        error: SourceResolutionError,
        telemetry: &RequestTelemetry,
    ) -> Self
    where
        C: Send + Sync + 'static,
    {
        match error {
            SourceResolutionError::Reject(rejection) => Self {
                result: LifecycleResult::Rejected,
                code: None,
                response: SourceFailureResponse::Rejection(rejection),
            },
            SourceResolutionError::Protocol(error) => Self {
                result: LifecycleResult::Rejected,
                code: None,
                response: SourceFailureResponse::GraphQl {
                    status: 200,
                    error: *error,
                },
            },
            SourceResolutionError::Unavailable(error) => Self {
                result: LifecycleResult::Failed,
                code: Some("PERSISTED_OPERATION_UNAVAILABLE"),
                response: SourceFailureResponse::GraphQl {
                    status: 503,
                    error: observed_error_value(
                        schema,
                        &error,
                        LifecyclePhase::OperationSourceResolution,
                        "operation-source",
                        telemetry,
                        "Persisted operation service is unavailable",
                        "PERSISTED_OPERATION_UNAVAILABLE",
                    ),
                },
            },
            SourceResolutionError::Internal(error) => Self {
                result: LifecycleResult::Failed,
                code: Some("INTERNAL_SERVER_ERROR"),
                response: SourceFailureResponse::GraphQl {
                    status: 500,
                    error: observed_error_value(
                        schema,
                        &error,
                        LifecyclePhase::OperationSourceResolution,
                        "operation-source",
                        telemetry,
                        "Internal server error",
                        "INTERNAL_SERVER_ERROR",
                    ),
                },
            },
        }
    }

    pub(crate) const fn lifecycle(&self) -> (LifecycleResult, Option<&'static str>) {
        (self.result, self.code)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn into_graphql_response(self) -> GraphQlResponse {
        match self.response {
            SourceFailureResponse::Rejection(rejection) => {
                GraphQlResponse::request_error(rejection.error().clone())
            }
            SourceFailureResponse::GraphQl { error, .. } => GraphQlResponse::request_error(error),
        }
    }

    fn into_http_response(self, media: ResponseMediaType) -> ResponseParts {
        match self.response {
            SourceFailureResponse::Rejection(rejection) => rejection.response_parts(media),
            SourceFailureResponse::GraphQl { status, error } => {
                error_value_response(media, status, error)
            }
        }
    }
}

#[derive(Debug)]
struct OperationSourceTimeout;

impl std::fmt::Display for OperationSourceTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("operation source provider timed out")
    }
}

impl std::error::Error for OperationSourceTimeout {}
