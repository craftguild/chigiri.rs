//! Existing local setup path for runtime-neutral callers.

use chigiri_http::{
    BoxFuture, RequestPhase, ResponseCompletionOutcome, ResponseHeaders, SubscriptionTransport,
};
use chigiri_runtime::__private::SubscriptionStart;
use chigiri_runtime::LifecycleResult;
use chigiri_runtime::{GraphQlResponse, LifecyclePhase, Object};

use super::super::input::{PreparedEndpointSubscription, ValidatedEndpointSubscription};
use super::super::{EndpointSubscription, SubscriptionCompletion, SubscriptionInitialization};
use crate::subscription::preparation::initial::{
    InitialResponsePreparation, prepare_initial_response,
};
use crate::subscription::preparation::{ContextPreparation, prepare_context};
use crate::transport::observation::Phase;
use crate::transport::response::{internal_error_response, timeout_response};
use crate::transport::run;

pub(crate) fn initialize<C: Send + Sync + 'static>(
    input: PreparedEndpointSubscription<'_, C>,
) -> BoxFuture<'_, SubscriptionInitialization<C>> {
    let PreparedEndpointSubscription {
        validated,
        mut timers,
    } = input;
    let telemetry = validated.prepared.setup_telemetry().cloned();
    let instrumentation = telemetry
        .as_ref()
        .and_then(chigiri_runtime::RequestTelemetry::instrumentation_scope)
        .cloned();
    let ValidatedEndpointSubscription {
        state,
        prepared,
        request,
    } = validated;
    let deadlines = timers.factory;
    chigiri_runtime::instrumentation::instrument_future(instrumentation, async move {
        let media = request.media();
        let profile = request.transport();
        let (fresh, started) = {
            let mut combined =
                Box::pin(std::future::poll_fn(|context| timers.poll_expired(context)));
            let fresh = match prepare_context(
                &state.schema,
                &state.context_factory,
                &request,
                RequestPhase::SubscriptionSetup { transport: profile },
                deadlines,
                state.settings.context_timeout,
                combined.as_mut(),
                telemetry.as_ref(),
            )
            .await
            {
                ContextPreparation::Ready(fresh) => fresh,
                ContextPreparation::Rejected {
                    rejection,
                    telemetry,
                } => {
                    drop(telemetry);
                    return SubscriptionInitialization::Rejected(rejection.response_parts(media));
                }
                ContextPreparation::Internal { error, telemetry } => {
                    return SubscriptionInitialization::Rejected(internal_error_response(
                        &state.schema,
                        media,
                        &error,
                        LifecyclePhase::ContextConstruction,
                        "subscription-context",
                        &telemetry,
                    ));
                }
                ContextPreparation::TimedOut { telemetry } => {
                    drop(telemetry);
                    return SubscriptionInitialization::Rejected(timeout_response(media));
                }
            };
            let started = run(
                deadlines,
                state.settings.execution_timeout,
                combined.as_mut(),
                prepared.start(fresh.execution_context(), Some(deadlines)),
            )
            .await;
            (fresh, started)
        };
        let started = match started {
            Ok(started) => started,
            Err(_) => {
                let completion = fresh.into_failure_completion(profile);
                return SubscriptionInitialization::Rejected(
                    completion.finish(timeout_response(media), ResponseCompletionOutcome::TimedOut),
                );
            }
        };
        let (session, response) = match started {
            SubscriptionStart::Ready(session) => (
                Some(*session),
                GraphQlResponse {
                    data: None,
                    errors: Vec::new(),
                    extensions: Object::new(),
                },
            ),
            SubscriptionStart::Response(response) => (None, response),
        };
        let initial = {
            let mut combined =
                Box::pin(std::future::poll_fn(|context| timers.poll_expired(context)));
            let hook_telemetry = fresh.telemetry().clone();
            let hook_phase = (profile != SubscriptionTransport::WebSocket).then(|| {
                Phase::start(&state.schema, LifecyclePhase::ResponseHook, &hook_telemetry)
            });
            let initial = prepare_initial_response(
                fresh,
                &request,
                &response,
                state.response_hook.as_ref(),
                deadlines,
                state.settings.response_hook_timeout,
                combined.as_mut(),
            );
            let initial = match hook_phase.as_ref() {
                Some(phase) => phase.instrument(initial).await,
                None => initial.await,
            };
            if let Some(phase) = hook_phase {
                let (result, code) = match &initial {
                    InitialResponsePreparation::HookFailed { .. } => {
                        (LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))
                    }
                    InitialResponsePreparation::TimedOut { .. } => {
                        (LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"))
                    }
                    _ => (LifecycleResult::Succeeded, None),
                };
                phase.finish(result, code);
            }
            initial
        };
        let (headers, completion) = match initial {
            InitialResponsePreparation::Ready(prepared) => {
                let (headers, completion, _) = prepared.into_parts();
                (headers, SubscriptionCompletion::armed(completion))
            }
            InitialResponsePreparation::WebSocket { telemetry } => {
                drop(telemetry);
                (ResponseHeaders::new(), SubscriptionCompletion::empty())
            }
            InitialResponsePreparation::HookFailed {
                error,
                completion,
                telemetry,
            } => {
                drop(session);
                let response = internal_error_response(
                    &state.schema,
                    media,
                    &error,
                    LifecyclePhase::ResponseHook,
                    "subscription-response-hook",
                    &telemetry,
                );
                return SubscriptionInitialization::Rejected(
                    completion.finish(response, ResponseCompletionOutcome::Failed),
                );
            }
            InitialResponsePreparation::TimedOut {
                completion,
                telemetry,
            } => {
                drop(telemetry);
                drop(session);
                return SubscriptionInitialization::Rejected(
                    completion.finish(timeout_response(media), ResponseCompletionOutcome::TimedOut),
                );
            }
        };
        match session {
            Some(session) => SubscriptionInitialization::Ready {
                subscription: Box::new(EndpointSubscription {
                    state,
                    request,
                    session: Some(session),
                    sequence: 0,
                }),
                headers,
                completion,
            },
            None => SubscriptionInitialization::Response {
                response,
                headers,
                completion,
            },
        }
    })
}
