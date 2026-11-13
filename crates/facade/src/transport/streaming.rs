//! Shared HTTP ingress and the owned pre-worker subscription handoff.

mod observation;
#[cfg(feature = "subscription-worker")]
pub(crate) mod supervisor;
#[cfg(test)]
mod tests;
#[cfg(feature = "subscription-worker")]
mod worker;

use std::sync::Arc;

use chigiri_http::{
    BoxFuture, DeadlineFactory, RequestBody, ResponseParts, SubscriptionTransport,
    TransportRequest, negotiate_response_media,
};
use chigiri_runtime::__private::SelectedHttpOperation;
use chigiri_runtime::{LifecyclePhase, LifecycleResult};

use crate::subscription::bridge_resources::{BridgeResourceError, PendingBridgeOperation};
use crate::subscription::driver::input::{SubscriptionPreparation, capture};
use crate::transport::ingress::{HttpIngressMode, ReceivedHttpRequest, receive};
use crate::transport::response::{error_response, internal_error_response, timeout_response};
use crate::transport::{EndpointState, InFlightRequest};

pub(crate) use observation::InitialRequestObservation;
#[cfg(feature = "subscription-worker")]
pub use worker::{
    HttpSubscriptionEvents, HttpSubscriptionPreparation, HttpSubscriptionSignal,
    PreparedHttpSubscription, ReadyHttpSubscription, SubscriptionInitialResponse,
};
#[cfg(all(test, feature = "subscription-worker"))]
pub(crate) use worker::{InitialCommitTestBarrier, InitialCommitTestFault};
#[cfg(all(test, feature = "subscription-worker"))]
pub(crate) use worker::{InitialWorkerProbe, InitialWorkerStage};

/// One finite response or a bounded, admitted input awaiting its official worker.
#[doc(hidden)]
pub enum SubscriptionHttpIngress<'deadline, C: Send + Sync + 'static> {
    /// A complete pre-stream response or ordinary Query/Mutation result.
    Finite(ResponseParts),
    /// The sole pending-generation owner; no validation or source task exists yet.
    Pending(Box<PendingHttpSubscription<'deadline, C>>),
}

/// Owned HTTP subscription input with no borrowed request/body or EndpointState.
#[doc(hidden)]
pub struct PendingHttpSubscription<'deadline, C: Send + Sync + 'static> {
    pub(crate) preparation: SubscriptionPreparation<'deadline, C>,
    pub(crate) operation: PendingBridgeOperation,
    pub(crate) observation: InitialRequestObservation<C>,
    profile: SubscriptionTransport,
    #[cfg(feature = "subscription-worker")]
    worker_configuration: worker::HttpWorkerConfiguration,
}

impl<C: Send + Sync + 'static> PendingHttpSubscription<'_, C> {
    /// Returns the already selected SSE or multipart transport profile.
    pub const fn transport(&self) -> SubscriptionTransport {
        self.profile
    }

    /// Cancels before worker creation, releasing input, quota, and observation.
    pub fn cancel(self) {
        let Self {
            preparation,
            operation,
            observation,
            profile: _,
            #[cfg(feature = "subscription-worker")]
                worker_configuration: _,
        } = self;
        drop(preparation);
        drop(operation);
        drop(observation);
    }
}

pub(crate) fn handle<'a, 'deadline: 'a, C, B>(
    state: Arc<EndpointState<C>>,
    transport: TransportRequest<'a>,
    body: B,
    deadlines: &'deadline dyn DeadlineFactory,
) -> BoxFuture<'a, SubscriptionHttpIngress<'deadline, C>>
where
    C: Send + Sync + 'static,
    B: RequestBody + 'a,
{
    handle_monitored(
        state,
        transport,
        body,
        deadlines,
        super::deadline::HttpDeadlineMonitor::default(),
    )
}

pub(crate) fn handle_monitored<'a, 'deadline: 'a, C, B>(
    state: Arc<EndpointState<C>>,
    transport: TransportRequest<'a>,
    mut body: B,
    deadlines: &'deadline dyn DeadlineFactory,
    monitor: super::deadline::HttpDeadlineMonitor,
) -> BoxFuture<'a, SubscriptionHttpIngress<'deadline, C>>
where
    C: Send + Sync + 'static,
    B: RequestBody + 'a,
{
    use chigiri_runtime::instrumentation::{InstrumentationKind, instrument_future};

    let captured = transport
        .__instrumentation_capture()
        .cloned()
        .or_else(|| state.schema.capture_instrumentation(&transport));
    let handoff = captured
        .as_ref()
        .map(|value| value.handoff_scope(InstrumentationKind::Request));
    instrument_future(handoff, async move {
        let in_flight = InFlightRequest::start(Arc::clone(&state.in_flight));
        let telemetry = state.schema.request_telemetry_with_capture(
            transport.trace_id().cloned(),
            captured.as_ref(),
            InstrumentationKind::Request,
        );
        let scope = telemetry.instrumentation_scope().cloned();
        let work = async move {
            let observation =
                InitialRequestObservation::new(state.schema.clone(), telemetry.clone(), in_flight);
            let transport = transport
                .__with_request_telemetry(&telemetry)
                .__with_deadline_factory(deadlines);
            let media = negotiate_response_media(transport.headers())
                .unwrap_or(chigiri_http::ResponseMediaType::GraphQlResponseJson);
            if state.settings.request_timeout.is_zero() {
                return finite(timeout_response(media), observation);
            }
            // This owner starts before admission/body/source/hooks and moves unchanged.
            let mut overall = monitor.overall(deadlines, state.settings.request_timeout);
            let received = match receive(
                &state,
                transport,
                &mut body,
                deadlines,
                overall.as_mut(),
                &telemetry,
                HttpIngressMode::StreamingAware,
                &monitor,
            )
            .await
            {
                Ok(received) => received,
                Err(response) => return finite(response, observation),
            };
            dispatch(
                &state,
                transport,
                received,
                deadlines,
                overall,
                observation,
                &monitor,
            )
            .await
        };
        match scope {
            Some(scope) => scope.instrument(work).await,
            None => work.await,
        }
    })
}

async fn dispatch<'deadline, C: Send + Sync + 'static>(
    state: &EndpointState<C>,
    transport: TransportRequest<'_>,
    received: ReceivedHttpRequest,
    deadlines: &'deadline dyn DeadlineFactory,
    mut overall: BoxFuture<'deadline, ()>,
    observation: InitialRequestObservation<C>,
    monitor: &super::deadline::HttpDeadlineMonitor,
) -> SubscriptionHttpIngress<'deadline, C> {
    let ReceivedHttpRequest {
        graph_request,
        raw_body,
        negotiation,
    } = received;
    let media = negotiation.provisional_media();
    let method = negotiation.method();
    let selected = state.schema.__selected_http_operation(&graph_request);
    // An indeterminate parse/selection never authorizes a source. A client
    // accepting only a supported stream profile can still receive its bounded
    // request-error representation; full preparation must reject before setup.
    // If finite media is acceptable, retain the ordinary finite error path.
    let stream_only_error = selected == SelectedHttpOperation::Indeterminate
        && negotiation.finite_media().is_err()
        && matches!(negotiation.subscription_profile(), Some(Ok(_)));
    if selected != SelectedHttpOperation::Subscription && !stream_only_error {
        let media = match negotiation.finite_media() {
            Ok(media) => media,
            Err(error) => return finite(error.into_response_parts(), observation),
        };
        if !monitor.admit_finite() {
            return finite(
                ResponseParts {
                    status: 503,
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                observation,
            );
        }
        let response = super::execution::execute(
            state,
            deadlines,
            overall.as_mut(),
            transport.__with_response_media(media),
            graph_request,
            method,
            media,
            &raw_body,
            monitor,
        )
        .await;
        return finite(response, observation);
    }
    let profile = match negotiation.subscription_profile() {
        Some(Ok(profile)) => profile,
        Some(Err(error)) => return finite(error.into_response_parts(), observation),
        None => {
            return admission_failure(
                &state.schema,
                media,
                BridgeResourceError::Invariant,
                observation,
            );
        }
    };
    let pending = match state.subscriptions.resources.begin_http_subscription() {
        Ok(pending) => pending,
        Err(error) => return admission_failure(&state.schema, media, error, observation),
    };
    let preparation = match capture(
        Arc::clone(&state.subscription_execution),
        transport,
        graph_request,
        method,
        media,
        profile,
        None,
        deadlines,
        Some(overall),
    ) {
        Ok(preparation) => preparation,
        Err(_) => {
            drop(pending);
            let response = error_response(
                media,
                413,
                "Subscriber request metadata exceeds its limit",
                "BAD_USER_INPUT",
            );
            observation.finish(LifecycleResult::Rejected, Some("BAD_USER_INPUT"));
            return SubscriptionHttpIngress::Finite(response);
        }
    };
    drop(raw_body);
    SubscriptionHttpIngress::Pending(Box::new(PendingHttpSubscription {
        preparation,
        operation: pending,
        observation,
        profile,
        #[cfg(feature = "subscription-worker")]
        worker_configuration: worker::HttpWorkerConfiguration::new(
            media,
            state.subscriptions.resources.delivery().byte_owner(),
            state.subscriptions.settings,
        ),
    }))
}

fn finite<C: Send + Sync + 'static>(
    response: ResponseParts,
    observation: InitialRequestObservation<C>,
) -> SubscriptionHttpIngress<'static, C> {
    observation.finish_status(response.status);
    SubscriptionHttpIngress::Finite(response)
}

fn admission_failure<C: Send + Sync + 'static>(
    schema: &chigiri_runtime::ExecutableSchema<C>,
    media: chigiri_http::ResponseMediaType,
    error: BridgeResourceError,
    observation: InitialRequestObservation<C>,
) -> SubscriptionHttpIngress<'static, C> {
    if error == BridgeResourceError::Capacity {
        let response = error_response(
            media,
            503,
            "Subscription capacity is unavailable",
            "SUBSCRIPTION_LIMIT_EXCEEDED",
        );
        observation.finish(
            LifecycleResult::Rejected,
            Some("SUBSCRIPTION_LIMIT_EXCEEDED"),
        );
        return SubscriptionHttpIngress::Finite(response);
    }
    let response = internal_error_response(
        schema,
        media,
        &error,
        LifecyclePhase::TransportAdmission,
        "subscription-admission",
        observation.telemetry(),
    );
    observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
    SubscriptionHttpIngress::Finite(response)
}
