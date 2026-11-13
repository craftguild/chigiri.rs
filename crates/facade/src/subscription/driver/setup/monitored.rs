//! Setup execution whose exact timers are polled by the outer worker.

use chigiri_http::{
    BoxFuture, RequestPhase, ResponseCompletionOutcome, ResponseHeaders, SubscriptionTransport,
};
use chigiri_runtime::__private::SubscriptionStart;
use chigiri_runtime::LifecycleResult;
use chigiri_runtime::{GraphQlResponse, LifecyclePhase, Object};

use super::super::input::ValidatedEndpointSubscription;
use super::super::{EndpointSubscription, SubscriptionCompletion, SubscriptionInitialization};
use super::source::{MonitoredSourceStart, start_source_monitored};
use crate::subscription::preparation::initial::{
    InitialResponsePreparation, prepare_initial_response, prepare_initial_response_monitored,
};
use crate::subscription::preparation::{ContextPreparation, prepare_initial_context_monitored};
use crate::subscription::worker::{DeadlineArmer, DeadlineRole, SubscriptionWorkerRuntimeError};
use crate::transport::observation::Phase;
use crate::transport::response::{internal_error_response, timeout_response};

pub(crate) fn initialize_monitored<'a, C: Send + Sync + 'static>(
    validated: ValidatedEndpointSubscription<C>,
    armer: &DeadlineArmer,
    owner: &'a mut crate::subscription::worker::InitialTaskOwner<C>,
    handoff: &'a dyn crate::subscription::worker::observation::HostObservationHandoff,
) -> BoxFuture<'a, Result<SubscriptionInitialization<C>, SubscriptionWorkerRuntimeError>> {
    let telemetry = validated.prepared.setup_telemetry().cloned();
    let instrumentation = telemetry
        .as_ref()
        .and_then(chigiri_runtime::RequestTelemetry::instrumentation_scope)
        .cloned();
    let ValidatedEndpointSubscription {
        state,
        mut prepared,
        mut request,
    } = validated;
    let armer = armer.clone();
    chigiri_runtime::instrumentation::instrument_future(instrumentation, async move {
        let armer = armer.with_instrumentation(
            state
                .schema
                .capture_instrumentation(&())
                .map(|captured| captured.retain_within(0)),
        );
        let deadlines = armer.factory();
        let media = request.media();
        let profile = request.transport();
        let mut inherited = armer.setup_pair()?;
        let mut context_deadline =
            armer.arm(DeadlineRole::Phase, state.settings.context_timeout)?;
        if !state.authorize_start() {
            return Err(state.setup_stopped_error());
        }
        let fresh = prepare_initial_context_monitored(
            &state.schema,
            &state.context_factory,
            &request,
            RequestPhase::SubscriptionSetup { transport: profile },
            deadlines,
            &mut context_deadline,
            &mut inherited,
            owner,
            telemetry.as_ref(),
        )
        .await?;
        let fresh = match fresh {
            ContextPreparation::Ready(fresh) => fresh,
            ContextPreparation::Rejected {
                rejection,
                telemetry,
            } => {
                drop(telemetry);
                context_deadline.clear()?;
                finish_initial_deadlines(profile, inherited)?;
                return Ok(SubscriptionInitialization::Rejected(
                    rejection.response_parts(media),
                ));
            }
            ContextPreparation::Internal { error, telemetry } => {
                context_deadline.clear()?;
                finish_initial_deadlines(profile, inherited)?;
                return Ok(SubscriptionInitialization::Rejected(
                    internal_error_response(
                        &state.schema,
                        media,
                        &error,
                        LifecyclePhase::ContextConstruction,
                        "subscription-context",
                        &telemetry,
                    ),
                ));
            }
            ContextPreparation::TimedOut { telemetry } => {
                drop(telemetry);
                return Ok(SubscriptionInitialization::CompletionResponse {
                    response: timeout_response(media),
                    completion: SubscriptionCompletion::empty(),
                    outcome: ResponseCompletionOutcome::TimedOut,
                });
            }
        };
        context_deadline.clear()?;
        if !state.authorize_start() {
            return Err(state.setup_stopped_error());
        }
        if let Err(response) = crate::subscription::worker::observation::transfer(
            &mut prepared,
            fresh.execution_context(),
            &mut request,
            state.subscriptions.limits().max_retained_metadata_bytes,
            handoff,
        ) {
            finish_initial_deadlines(profile, inherited)?;
            return Ok(SubscriptionInitialization::Response {
                response: *response,
                headers: ResponseHeaders::new(),
                completion: SubscriptionCompletion::empty(),
            });
        }
        if !state.authorize_start() {
            return Err(state.setup_stopped_error());
        }
        let started = start_source_monitored(
            prepared,
            fresh.execution_context(),
            state.settings.execution_timeout,
            &armer,
            &mut inherited,
        )
        .await?;
        let (started, source_deadline) = match started {
            MonitoredSourceStart::Completed { started, deadline } => (started, deadline),
            MonitoredSourceStart::TimedOut => {
                let completion = fresh.into_failure_completion(profile);
                return Ok(SubscriptionInitialization::CompletionResponse {
                    response: timeout_response(media),
                    completion: SubscriptionCompletion::armed(completion),
                    outcome: ResponseCompletionOutcome::TimedOut,
                });
            }
        };
        let response = match started {
            SubscriptionStart::Ready(session) => {
                owner.setup_source(session);
                GraphQlResponse {
                    data: None,
                    errors: Vec::new(),
                    extensions: Object::new(),
                }
            }
            SubscriptionStart::Response(response) => response,
        };
        source_deadline.clear()?;
        if !state.authorize_start() {
            return Err(state.setup_stopped_error());
        }
        let hook_telemetry = fresh.telemetry().clone();
        let hook_phase = (profile != SubscriptionTransport::WebSocket)
            .then(|| Phase::start(&state.schema, LifecyclePhase::ResponseHook, &hook_telemetry));
        let initial = async {
            let value =
                if profile == SubscriptionTransport::WebSocket || state.response_hook.is_none() {
                    let mut inherited_ref = Box::pin(&mut inherited);
                    prepare_initial_response(
                        fresh,
                        &request,
                        &response,
                        state.response_hook.as_ref(),
                        deadlines,
                        state.settings.response_hook_timeout,
                        inherited_ref.as_mut(),
                    )
                    .await
                } else {
                    let mut hook_deadline =
                        armer.arm(DeadlineRole::Phase, state.settings.response_hook_timeout)?;
                    let initial = prepare_initial_response_monitored(
                        fresh,
                        &request,
                        &response,
                        state.response_hook.as_ref(),
                        deadlines,
                        &mut hook_deadline,
                        &mut inherited,
                    )
                    .await;
                    if !matches!(initial, InitialResponsePreparation::TimedOut { .. }) {
                        hook_deadline.clear()?;
                    }
                    initial
                };
            Ok::<_, SubscriptionWorkerRuntimeError>(value)
        };
        let initial = match hook_phase.as_ref() {
            Some(phase) => phase.instrument(initial).await?,
            None => initial.await?,
        };
        finish_hook_phase(hook_phase, &initial);
        if !state.authorize_start() {
            return Err(state.setup_stopped_error());
        }
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
                owner.discard_source();
                let response = internal_error_response(
                    &state.schema,
                    media,
                    &error,
                    LifecyclePhase::ResponseHook,
                    "subscription-response-hook",
                    &telemetry,
                );
                finish_initial_deadlines(profile, inherited)?;
                return Ok(SubscriptionInitialization::CompletionResponse {
                    response,
                    completion: SubscriptionCompletion::armed(completion),
                    outcome: ResponseCompletionOutcome::Failed,
                });
            }
            InitialResponsePreparation::TimedOut {
                completion,
                telemetry,
            } => {
                drop(telemetry);
                owner.discard_source();
                return Ok(SubscriptionInitialization::CompletionResponse {
                    response: timeout_response(media),
                    completion: SubscriptionCompletion::armed(completion),
                    outcome: ResponseCompletionOutcome::TimedOut,
                });
            }
        };
        finish_initial_deadlines(profile, inherited)?;
        Ok(match owner.take_setup_source() {
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
        })
    })
}

fn finish_initial_deadlines(
    profile: SubscriptionTransport,
    inherited: crate::subscription::worker::MonitoredDeadlinePair,
) -> Result<(), SubscriptionWorkerRuntimeError> {
    if profile == SubscriptionTransport::WebSocket {
        inherited.clear()
    } else {
        inherited.handoff_overall_and_clear_setup()
    }
}

fn finish_hook_phase<C>(
    phase: Option<crate::transport::observation::Phase<'_, C>>,
    initial: &InitialResponsePreparation,
) where
    C: Send + Sync + 'static,
{
    let Some(phase) = phase else {
        return;
    };
    let (result, code) = match initial {
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
