//! Owned preparation checkpoints with deadlines started before worker transfer.

use std::sync::Arc;

use chigiri_http::{
    BoxFuture, DeadlineFactory, RequestMethod, ResponseMediaType, SubscriptionTransport,
    TransportRequest,
};
use chigiri_runtime::__private::PreparedSubscription;
#[cfg(feature = "subscription-worker")]
use chigiri_runtime::__private::PreparedWebSocketOperation;
use chigiri_runtime::{GraphQlError, GraphQlRequest, GraphQlResponse, Object};

use super::setup_deadlines::SubscriptionSetupDeadlines;
use super::{SubscriptionInitialization, setup};
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::subscription::request::{MetadataRetentionLimit, SubscriberIngress, SubscriberRequest};

#[cfg(feature = "subscription-worker")]
mod websocket;
#[cfg(feature = "subscription-worker")]
pub(crate) use websocket::{
    WebSocketOperationIngress, WebSocketValidationInput, WebSocketValidationTaskOutput,
};

/// Bounded owned input; validation may invoke application custom scalar callbacks.
pub(crate) struct SubscriptionPreparation<'deadline, C> {
    validation: SubscriptionValidation<C>,
    timers: SubscriptionSetupDeadlines<'deadline>,
}

/// Only resource-free input enters the synchronous validation task.
pub(crate) struct SubscriptionValidation<C> {
    state: Arc<SubscriptionExecutionState<C>>,
    request: GraphQlRequest,
    ingress: SubscriberIngress,
}

/// Validation output has no timers, endpoint resources, or source owner.
pub(crate) struct ValidatedEndpointSubscription<C> {
    pub(super) state: Arc<SubscriptionExecutionState<C>>,
    pub(super) prepared: PreparedSubscription<C>,
    pub(super) request: SubscriberRequest,
}

/// Resource-free WebSocket input before full validation and exact-kind admission.
#[cfg(feature = "subscription-worker")]
pub(crate) struct WebSocketOperationValidation<C> {
    state: Arc<SubscriptionExecutionState<C>>,
    request: GraphQlRequest,
    ingress: SubscriberIngress,
}

/// Joined WebSocket validation result with no timer or endpoint owner.
#[cfg(feature = "subscription-worker")]
pub(crate) struct ValidatedWebSocketOperation<C> {
    pub(crate) state: Arc<SubscriptionExecutionState<C>>,
    pub(crate) prepared: PreparedWebSocketOperation<C>,
    pub(crate) request: SubscriberRequest,
}

/// Validated owned preparation awaiting active admission, not a source owner.
pub(crate) struct PreparedEndpointSubscription<'deadline, C> {
    pub(super) validated: ValidatedEndpointSubscription<C>,
    pub(super) timers: SubscriptionSetupDeadlines<'deadline>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn capture<'deadline, C>(
    state: Arc<SubscriptionExecutionState<C>>,
    transport: TransportRequest<'_>,
    request: GraphQlRequest,
    method: RequestMethod,
    media: ResponseMediaType,
    profile: SubscriptionTransport,
    authorized_init: Option<Object>,
    deadlines: &'deadline dyn DeadlineFactory,
    overall: Option<BoxFuture<'deadline, ()>>,
) -> Result<SubscriptionPreparation<'deadline, C>, Box<SubscriptionInitialization<C>>> {
    let overall = overall.unwrap_or_else(|| deadlines.sleep(state.settings.request_timeout));
    let setup_deadline = deadlines.sleep(state.subscriptions.limits().setup_timeout);
    let ingress = SubscriberIngress::capture(
        transport,
        &request,
        method,
        media,
        profile,
        authorized_init,
        MetadataRetentionLimit::from_validated(&state.subscriptions),
    )
    .map_err(|_| metadata_response())?;
    Ok(SubscriptionPreparation {
        validation: SubscriptionValidation {
            state,
            request,
            ingress,
        },
        timers: SubscriptionSetupDeadlines {
            factory: deadlines,
            overall,
            setup: setup_deadline,
        },
    })
}

impl<'deadline, C: Send + Sync + 'static> SubscriptionPreparation<'deadline, C> {
    /// Keeps the deadline owners outside the task that may block in coercion.
    pub(crate) fn into_validation(
        self,
    ) -> (
        SubscriptionValidation<C>,
        SubscriptionSetupDeadlines<'deadline>,
    ) {
        (self.validation, self.timers)
    }

    /// Local non-worker entry; official workers use the split ownership above.
    pub(crate) fn prepare(
        self,
        captured: Option<&chigiri_runtime::instrumentation::CapturedInstrumentation>,
    ) -> Result<PreparedEndpointSubscription<'deadline, C>, Box<SubscriptionInitialization<C>>>
    {
        let (mut validation, timers) = self.into_validation();
        if validation.request.request_telemetry().is_none() {
            validation.request.telemetry =
                Some(validation.state.schema.request_telemetry_with_capture(
                    validation.ingress.trace_id().cloned(),
                    captured,
                    chigiri_runtime::instrumentation::InstrumentationKind::Request,
                ));
        } else {
            validation
                .state
                .schema
                .prepare_request_telemetry(&mut validation.request, captured);
        }
        let scope = validation
            .request
            .request_telemetry()
            .and_then(chigiri_runtime::RequestTelemetry::instrumentation_scope)
            .cloned();
        let prepared = match scope {
            Some(scope) => scope.run(|| validation.prepare()),
            None => validation.prepare(),
        };
        prepared.map(|validated| validated.with_deadlines(timers))
    }
}

impl<C: Send + Sync + 'static> SubscriptionValidation<C> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn instrumentation(
        &self,
    ) -> Option<chigiri_runtime::instrumentation::InstrumentationScope> {
        self.request
            .request_telemetry()
            .and_then(chigiri_runtime::RequestTelemetry::instrumentation_scope)
            .cloned()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn worker_scope(
        &self,
    ) -> (
        crate::managed::EventActivity,
        Option<crate::managed::HttpFaultReporter>,
    ) {
        self.state.task_scope()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn prepare_worker(
        self,
    ) -> Result<
        Result<ValidatedEndpointSubscription<C>, Box<SubscriptionInitialization<C>>>,
        crate::subscription::worker::SubscriptionWorkerRuntimeError,
    > {
        if !self.state.authorize_start() {
            return Err(self.state.setup_stopped_error());
        }
        Ok(self.prepare())
    }

    /// Must run under the exact pending worker charge, before active transfer.
    pub(crate) fn prepare(
        mut self,
    ) -> Result<ValidatedEndpointSubscription<C>, Box<SubscriptionInitialization<C>>> {
        if self.request.request_telemetry().is_none() {
            self.request.telemetry = Some(
                self.state
                    .schema
                    .request_telemetry(self.ingress.trace_id().cloned()),
            );
        }
        let mut prepared = self
            .state
            .schema
            .prepare_subscription(self.request)
            .map_err(|response| Box::new(setup::response_only(*response)))?;
        SubscriberRequest::budget_instrumentation(
            &self.ingress,
            &mut prepared,
            MetadataRetentionLimit::from_validated(&self.state.subscriptions),
        )
        .map_err(|_| metadata_response())?;
        let request = SubscriberRequest::from_ingress(
            self.ingress,
            &prepared,
            MetadataRetentionLimit::from_validated(&self.state.subscriptions),
        )
        .map_err(|_| metadata_response())?;
        Ok(ValidatedEndpointSubscription {
            state: self.state,
            prepared,
            request,
        })
    }
}

#[cfg(feature = "subscription-worker")]
impl<C: Send + Sync + 'static> WebSocketOperationValidation<C> {
    pub(crate) fn new(
        state: Arc<SubscriptionExecutionState<C>>,
        request: GraphQlRequest,
        ingress: SubscriberIngress,
    ) -> Self {
        Self {
            state,
            request,
            ingress,
        }
    }

    /// Runs full validation/coercion inside the charged pending worker.
    pub(crate) fn prepare(
        mut self,
    ) -> Result<ValidatedWebSocketOperation<C>, Box<GraphQlResponse>> {
        if self.request.request_telemetry().is_none() {
            self.request.telemetry = Some(
                self.state
                    .schema
                    .request_telemetry(self.ingress.trace_id().cloned()),
            );
        }
        let mut prepared = self
            .state
            .schema
            .prepare_websocket_operation(self.request)?;
        SubscriberRequest::budget_websocket_instrumentation(
            &self.ingress,
            &mut prepared,
            MetadataRetentionLimit::from_validated(&self.state.subscriptions),
        )
        .map_err(|_| Box::new(metadata_graphql_response()))?;
        let request = SubscriberRequest::from_websocket(
            self.ingress,
            &prepared,
            MetadataRetentionLimit::from_validated(&self.state.subscriptions),
        )
        .map_err(|_| Box::new(metadata_graphql_response()))?;
        Ok(ValidatedWebSocketOperation {
            state: self.state,
            prepared,
            request,
        })
    }
}

#[cfg(feature = "subscription-worker")]
pub(crate) fn metadata_graphql_response() -> GraphQlResponse {
    GraphQlResponse::request_error(
        GraphQlError::new("Subscriber request metadata exceeds its limit").code("BAD_USER_INPUT"),
    )
}

impl<C> ValidatedEndpointSubscription<C> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn retained_telemetry(&self) -> Option<&chigiri_runtime::RequestTelemetry> {
        self.request.request().request_telemetry()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn execution(&self) -> Arc<SubscriptionExecutionState<C>> {
        Arc::clone(&self.state)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn activity(&self) -> crate::managed::EventActivity {
        crate::managed::EventActivity::new(self.state.managed.get().cloned())
    }

    /// A worker attaches these only after its validation task has physically joined.
    pub(crate) fn with_deadlines(
        self,
        timers: SubscriptionSetupDeadlines<'_>,
    ) -> PreparedEndpointSubscription<'_, C> {
        PreparedEndpointSubscription {
            validated: self,
            timers,
        }
    }
}

impl<'deadline, C: Send + Sync + 'static> PreparedEndpointSubscription<'deadline, C> {
    /// The trusted owner must acquire active admission before invoking this phase.
    pub(crate) fn initialize(self) -> BoxFuture<'deadline, SubscriptionInitialization<C>> {
        setup::initialize(self)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn into_worker_parts(
        self,
    ) -> (
        ValidatedEndpointSubscription<C>,
        SubscriptionSetupDeadlines<'deadline>,
    ) {
        (self.validated, self.timers)
    }
}

fn metadata_response<C>() -> Box<SubscriptionInitialization<C>> {
    Box::new(setup::response_only(GraphQlResponse::request_error(
        GraphQlError::new("Subscriber request metadata exceeds its limit").code("BAD_USER_INPUT"),
    )))
}
