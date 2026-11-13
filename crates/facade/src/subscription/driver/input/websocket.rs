//! Bounded operation-source and decoded-hook input for one WebSocket operation.

use std::sync::Arc;

use chigiri_http::{
    AdmissionDecision, DecodedRequestView, HttpLifecycle, RequestEnvelope, RequestMethod,
    ResponseMediaType, TransportRequest,
};
use chigiri_runtime::instrumentation::{InstrumentationKind, InstrumentationScope};
use chigiri_runtime::{GraphQlResponse, LifecyclePhase, LifecycleResult, Object, RequestTelemetry};

use super::{ValidatedWebSocketOperation, WebSocketOperationValidation, metadata_graphql_response};
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::subscription::request::{
    MetadataRetentionLimit, RetentionError, WebSocketSubscriberIngressSeed,
};
use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, SubscriptionWorkerRuntimeError, run_monitored,
};
use crate::transport::response::observed_error_value;
use crate::transport::source::ObservedSourceFailure;
use crate::transport::{Phase, TimeoutKind};

/// Resource-free, pre-spawn input whose operation source is not resolved yet.
pub(crate) struct WebSocketOperationIngress<C> {
    state: Arc<SubscriptionExecutionState<C>>,
    lifecycle: HttpLifecycle,
    envelope: RequestEnvelope,
    ingress: WebSocketSubscriberIngressSeed,
}

/// Both accepted Phase-A inputs share one concrete worker and deadline table.
pub(crate) enum WebSocketValidationInput<C> {
    Resolved(WebSocketOperationValidation<C>),
    Ingress(WebSocketOperationIngress<C>),
}

pub(crate) enum WebSocketValidationTaskOutput<C> {
    Prepared(Box<ValidatedWebSocketOperation<C>>),
    Response(GraphQlResponse),
}

impl<C: Send + Sync + 'static> WebSocketOperationIngress<C> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture(
        state: Arc<SubscriptionExecutionState<C>>,
        lifecycle: HttpLifecycle,
        transport: TransportRequest<'_>,
        envelope: RequestEnvelope,
        telemetry: RequestTelemetry,
        authorized_init: Option<Arc<Object>>,
    ) -> Result<Self, RetentionError> {
        let captured = transport
            .__instrumentation_capture()
            .cloned()
            .or_else(|| state.schema.capture_instrumentation(&transport));
        let telemetry = match captured {
            Some(captured) => captured.start(telemetry, InstrumentationKind::Request),
            None => telemetry,
        };
        let envelope = envelope.with_request_telemetry(telemetry);
        let ingress = WebSocketSubscriberIngressSeed::capture(
            transport,
            &envelope,
            authorized_init,
            MetadataRetentionLimit::from_validated(&state.subscriptions),
        )?;
        Ok(Self {
            state,
            lifecycle,
            envelope,
            ingress,
        })
    }
}

impl<C> From<WebSocketOperationValidation<C>> for WebSocketValidationInput<C> {
    fn from(value: WebSocketOperationValidation<C>) -> Self {
        Self::Resolved(value)
    }
}

impl<C> From<WebSocketOperationIngress<C>> for WebSocketValidationInput<C> {
    fn from(value: WebSocketOperationIngress<C>) -> Self {
        Self::Ingress(value)
    }
}

impl<C: Send + Sync + 'static> WebSocketValidationInput<C> {
    pub(crate) fn instrumentation(&self) -> Option<InstrumentationScope> {
        let telemetry = match self {
            Self::Resolved(validation) => validation.request.request_telemetry(),
            Self::Ingress(ingress) => ingress.envelope.request_telemetry(),
        };
        telemetry
            .and_then(RequestTelemetry::instrumentation_scope)
            .cloned()
    }

    pub(crate) fn worker_scope(
        &self,
    ) -> (
        crate::managed::EventActivity,
        Option<crate::managed::HttpFaultReporter>,
    ) {
        match self {
            Self::Resolved(validation) => validation.state.task_scope(),
            Self::Ingress(ingress) => ingress.state.task_scope(),
        }
    }

    pub(crate) async fn prepare(
        self,
        armer: DeadlineArmer,
    ) -> Result<WebSocketValidationTaskOutput<C>, SubscriptionWorkerRuntimeError> {
        let schema = match &self {
            Self::Resolved(validation) => &validation.state.schema,
            Self::Ingress(ingress) => &ingress.state.schema,
        };
        let armer = armer.with_instrumentation(
            schema
                .capture_instrumentation(&())
                .map(|captured| captured.retain_within(0)),
        );
        let instrumentation = self.instrumentation();
        let result = match self {
            Self::Resolved(validation) => {
                if !validation.state.authorize_start() {
                    Err(SubscriptionWorkerRuntimeError::EndpointClosed)
                } else {
                    Ok(match validation.prepare() {
                        Ok(prepared) => WebSocketValidationTaskOutput::Prepared(Box::new(prepared)),
                        Err(response) => WebSocketValidationTaskOutput::Response(*response),
                    })
                }
            }
            Self::Ingress(ingress) => ingress.prepare(armer).await,
        };
        if let Some(scope) = instrumentation {
            match &result {
                Ok(WebSocketValidationTaskOutput::Response(_)) => {
                    scope.finish(LifecycleResult::Rejected)
                }
                Err(_) => scope.finish(LifecycleResult::Failed),
                Ok(WebSocketValidationTaskOutput::Prepared(_)) => {}
            }
        }
        result
    }
}

impl<C: Send + Sync + 'static> WebSocketOperationIngress<C> {
    async fn prepare(
        self,
        armer: DeadlineArmer,
    ) -> Result<WebSocketValidationTaskOutput<C>, SubscriptionWorkerRuntimeError> {
        let mut setup = armer.setup()?;
        let result = self.prepare_with_setup(&armer, &mut setup).await;
        // Validation and active setup share the original outer budget. Dropping
        // a live proxy here would clear it before the active worker can borrow
        // the same generation, even though no source setup has run yet.
        setup.relinquish();
        result
    }

    async fn prepare_with_setup(
        self,
        armer: &DeadlineArmer,
        setup: &mut crate::subscription::worker::MonitoredDeadline,
    ) -> Result<WebSocketValidationTaskOutput<C>, SubscriptionWorkerRuntimeError> {
        let Self {
            state,
            lifecycle,
            envelope,
            ingress,
        } = self;
        let telemetry = envelope
            .request_telemetry()
            .cloned()
            .ok_or(SubscriptionWorkerRuntimeError::Invariant)?;
        let mut source_deadline = armer.arm(
            DeadlineRole::Phase,
            state.settings.source_resolution_timeout,
        )?;
        if !state.authorize_start() {
            return Err(SubscriptionWorkerRuntimeError::EndpointClosed);
        }
        let phase = Phase::start(
            &state.schema,
            LifecyclePhase::OperationSourceResolution,
            &telemetry,
        );
        let request = match state
            .pending_phase(run_monitored(
                &mut source_deadline,
                setup,
                phase.instrument(async { lifecycle.resolve_source(envelope).await }),
            ))
            .await
        {
            None => return Err(SubscriptionWorkerRuntimeError::EndpointClosed),
            Some(Ok(Ok(request))) => {
                source_deadline.clear()?;
                phase.finish(LifecycleResult::Succeeded, None);
                request
            }
            Some(Ok(Err(error))) => {
                source_deadline.clear()?;
                let failure = ObservedSourceFailure::new(&state.schema, error, &telemetry);
                let (result, code) = failure.lifecycle();
                phase.finish(result, code);
                return Ok(WebSocketValidationTaskOutput::Response(
                    failure.into_graphql_response(),
                ));
            }
            Some(Err(TimeoutKind::Phase | TimeoutKind::Overall)) => {
                phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
                return Err(SubscriptionWorkerRuntimeError::Shutdown);
            }
        };
        let ingress = match ingress.finish(&request) {
            Ok(ingress) => ingress,
            Err(_) => {
                return Ok(WebSocketValidationTaskOutput::Response(
                    metadata_graphql_response(),
                ));
            }
        };
        let mut decoded_deadline =
            armer.arm(DeadlineRole::Phase, state.settings.decoded_request_timeout)?;
        if !state.authorize_start() {
            return Err(SubscriptionWorkerRuntimeError::EndpointClosed);
        }
        let phase = Phase::start(
            &state.schema,
            LifecyclePhase::DecodedRequestHook,
            &telemetry,
        );
        let view = DecodedRequestView::new(
            &request,
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
        );
        match state
            .pending_phase(run_monitored(
                &mut decoded_deadline,
                setup,
                phase.instrument(async { lifecycle.inspect_decoded(view).await }),
            ))
            .await
        {
            None => return Err(SubscriptionWorkerRuntimeError::EndpointClosed),
            Some(Ok(Ok(AdmissionDecision::Continue))) => {
                decoded_deadline.clear()?;
                phase.finish(LifecycleResult::Succeeded, None);
            }
            Some(Ok(Ok(AdmissionDecision::Reject(rejection)))) => {
                decoded_deadline.clear()?;
                phase.finish(LifecycleResult::Rejected, None);
                return Ok(WebSocketValidationTaskOutput::Response(
                    GraphQlResponse::request_error(rejection.error().clone()),
                ));
            }
            Some(Ok(Err(error))) => {
                decoded_deadline.clear()?;
                phase.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
                let error = observed_error_value(
                    &state.schema,
                    &error,
                    LifecyclePhase::DecodedRequestHook,
                    "decoded-request-hook",
                    &telemetry,
                    "Internal server error",
                    "INTERNAL_SERVER_ERROR",
                );
                return Ok(WebSocketValidationTaskOutput::Response(
                    GraphQlResponse::request_error(error),
                ));
            }
            Some(Err(TimeoutKind::Phase | TimeoutKind::Overall)) => {
                phase.finish(LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT"));
                return Err(SubscriptionWorkerRuntimeError::Shutdown);
            }
        }
        if !state.authorize_start() {
            return Err(SubscriptionWorkerRuntimeError::EndpointClosed);
        }
        match WebSocketOperationValidation::new(state, request, ingress).prepare() {
            Ok(prepared) => Ok(WebSocketValidationTaskOutput::Prepared(Box::new(prepared))),
            Err(response) => Ok(WebSocketValidationTaskOutput::Response(*response)),
        }
    }
}
