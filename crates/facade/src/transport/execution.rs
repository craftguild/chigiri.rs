//! Combines borrowed decoded text, request context, execution, and response-hook encoding under one deadline.

mod completion;
use completion::FiniteCompletion;

use super::deadline::HttpDeadlineMonitor;
use super::observation::Phase;
use super::response::{bounded_internal_error_response, internal_error_response, timeout_response};
use super::{EndpointState, TimeoutKind};
use chigiri_http::{
    __encode_response_with_limit, DeadlineFactory, PrepareError, RequestContext, RequestMethod,
    ResponseMediaType, ResponseParts, TransportRequest,
};
use chigiri_runtime::{LifecyclePhase, LifecycleResult};

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute<C>(
    state: &EndpointState<C>,
    deadlines: &dyn DeadlineFactory,
    mut overall: std::pin::Pin<&mut (dyn std::future::Future<Output = ()> + Send)>,
    transport: TransportRequest<'_>,
    graph_request: chigiri_runtime::GraphQlRequest,
    method: RequestMethod,
    media: ResponseMediaType,
    raw_body: &[u8],
    monitor: &HttpDeadlineMonitor,
) -> ResponseParts
where
    C: Send + Sync + 'static,
{
    let request_context =
        RequestContext::__new(transport, &graph_request, method, media).__with_raw_body(raw_body);
    let telemetry = graph_request
        .request_telemetry()
        .expect("HTTP request will hold telemetry after decode");
    let mut completion = FiniteCompletion::new(None, monitor);
    let phase = Phase::start(
        &state.schema,
        LifecyclePhase::ContextConstruction,
        telemetry,
    );
    let preparation = phase
        .monitor(
            monitor,
            deadlines,
            state.settings.context_timeout,
            overall.as_mut(),
            || async {
                #[cfg(feature = "subscription-worker")]
                let mut prepared = state
                    .context_factory
                    .create_scoped(
                        request_context,
                        super::factory::ContextTaskScope::http(monitor),
                    )
                    .await?;
                #[cfg(not(feature = "subscription-worker"))]
                let mut prepared = state.context_factory.create(request_context).await?;
                // Retain a successful preparation before the monitor can reject
                // a late result. Cancellation never silently drops an armed token.
                completion.arm(if monitor.is_managed() {
                    prepared.completion.take()
                } else {
                    prepared.completion.clone()
                });
                Ok(prepared)
            },
        )
        .await;
    #[cfg(feature = "subscription-worker")]
    if monitor.faulted() {
        if let (Some(scope), Ok(prepared)) =
            (super::factory::ContextTaskScope::http(monitor), preparation)
        {
            scope.dispose_preparation(prepared);
        }
        phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
        return chigiri_http::ResponseParts {
            status: 500,
            headers: Vec::new(),
            body: Vec::new(),
        };
    }
    let prepared = match preparation {
        Ok(Ok(prepared)) => {
            phase.finish(LifecycleResult::Succeeded, None);
            prepared
        }
        Ok(Err(PrepareError::Reject(rejection))) => {
            phase.finish(LifecycleResult::Rejected, None);
            return rejection.response_parts(media);
        }
        Ok(Err(PrepareError::Internal(error))) => {
            phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
            return internal_error_response(
                &state.schema,
                media,
                &error,
                LifecyclePhase::ContextConstruction,
                "context-factory",
                telemetry,
            );
        }
        Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
            completion.timed_out();
            phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
            return timeout_response(media);
        }
    };
    let mut outcome = chigiri_http::ResponseCompletionOutcome::Failed;
    let response = async {
        let execution_request = graph_request.clone();
        let execution = async move {
            match method {
                RequestMethod::Get => {
                    state
                        .schema
                        .execute_query_only_http(
                            execution_request,
                            prepared.execution_context,
                            deadlines,
                        )
                        .await
                }
                RequestMethod::Post => {
                    state
                        .schema
                        .execute_http(execution_request, prepared.execution_context, deadlines)
                        .await
                }
            }
        };
        let response = match monitor
            .run(
                LifecyclePhase::Execution,
                deadlines,
                state.settings.execution_timeout,
                overall.as_mut(),
                || execution,
            )
            .await
        {
            Ok(response) => response,
            Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
                completion.timed_out();
                outcome = chigiri_http::ResponseCompletionOutcome::TimedOut;
                return timeout_response(media);
            }
        };
        let phase = Phase::start(&state.schema, LifecyclePhase::ResponseHook, telemetry);
        let additions = match &state.response_hook {
            Some(hook) => match phase
                .monitor(
                    monitor,
                    deadlines,
                    state.settings.response_hook_timeout,
                    overall.as_mut(),
                    || hook.call(request_context, &response, prepared.response_state.as_ref()),
                )
                .await
            {
                Ok(Ok(headers)) => {
                    phase.finish(LifecycleResult::Succeeded, None);
                    Some(headers)
                }
                Ok(Err(error)) => {
                    phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
                    return internal_error_response(
                        &state.schema,
                        media,
                        &error,
                        LifecyclePhase::ResponseHook,
                        "response-hook",
                        telemetry,
                    );
                }
                Err(TimeoutKind::Phase | TimeoutKind::Overall) => {
                    completion.timed_out();
                    outcome = chigiri_http::ResponseCompletionOutcome::TimedOut;
                    phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
                    return timeout_response(media);
                }
            },
            None => {
                phase.finish(LifecycleResult::Succeeded, None);
                None
            }
        };
        let phase = Phase::start(&state.schema, LifecyclePhase::HttpEncode, telemetry);
        match phase.run(|| {
            __encode_response_with_limit(media, &response, state.settings.response_byte_limit)
        }) {
            Ok(mut response) => {
                if let Some(additions) = additions {
                    response.headers.extend(additions.__into_values());
                }
                phase.finish(LifecycleResult::Succeeded, None);
                outcome = chigiri_http::ResponseCompletionOutcome::Encoded;
                response
            }
            Err(error) => {
                phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
                if matches!(
                    error,
                    chigiri_http::JsonConversionError::ResponseSizeLimit
                        | chigiri_http::JsonConversionError::NumberExpansionLimit
                ) {
                    outcome = chigiri_http::ResponseCompletionOutcome::EncodingLimitExceeded;
                }
                bounded_internal_error_response(
                    &state.schema,
                    media,
                    state.settings.response_byte_limit,
                    &error,
                    LifecyclePhase::HttpEncode,
                    "response-encode",
                    telemetry,
                )
            }
        }
    }
    .await;
    if !completion.has_token() {
        return response;
    }
    // Check synchronous work against the deadline before the next await point.
    let expired = outcome != chigiri_http::ResponseCompletionOutcome::TimedOut
        && std::future::poll_fn(|context| {
            std::task::Poll::Ready(overall.as_mut().poll(context).is_ready())
        })
        .await;
    let response = if expired {
        outcome = chigiri_http::ResponseCompletionOutcome::TimedOut;
        timeout_response(media)
    } else {
        response
    };
    completion.finish(response, outcome)
}
