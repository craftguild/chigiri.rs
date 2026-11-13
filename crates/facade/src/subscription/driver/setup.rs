//! Pre-source preparation and initial HTTP response ownership.

mod legacy;
#[cfg(feature = "subscription-worker")]
mod monitored;
#[cfg(feature = "subscription-worker")]
mod source;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chigiri_http::{
    BoxFuture, DeadlineFactory, RequestMethod, ResponseHeaders, ResponseMediaType,
    SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::instrumentation::{InstrumentationKind, instrument_future};
use chigiri_runtime::{GraphQlRequest, GraphQlResponse, Object, RequestTelemetry};

use super::input::capture;
use super::{SubscriptionCompletion, SubscriptionInitialization};
use crate::transport::EndpointState;

pub(super) use legacy::initialize;
#[cfg(feature = "subscription-worker")]
pub(crate) use monitored::initialize_monitored;
#[cfg(feature = "subscription-worker")]
pub(crate) use source::{MonitoredSourceStart, start_source_monitored};

#[allow(clippy::too_many_arguments)]
pub(crate) fn start<'a, 'deadline: 'a, C: Send + Sync + 'static>(
    state: Arc<EndpointState<C>>,
    transport: TransportRequest<'a>,
    mut request: GraphQlRequest,
    method: RequestMethod,
    media: ResponseMediaType,
    profile: SubscriptionTransport,
    authorized_init: Option<Object>,
    deadlines: &'a dyn DeadlineFactory,
    mut inherited_deadline: Option<Pin<&'a mut (dyn Future<Output = ()> + Send + 'deadline)>>,
) -> BoxFuture<'a, SubscriptionInitialization<C>> {
    // A pending source/setup must not retain the endpoint's quarantine registry.
    let state = Arc::clone(&state.subscription_execution);
    if request.request_telemetry().is_none() {
        request.telemetry = transport.request_telemetry().cloned();
    }
    let existing = request
        .request_telemetry()
        .and_then(RequestTelemetry::instrumentation_scope)
        .cloned();
    let captured = existing
        .is_none()
        .then(|| {
            transport
                .__instrumentation_capture()
                .cloned()
                .or_else(|| state.schema.capture_instrumentation(&transport))
        })
        .flatten();
    let handoff = existing.or_else(|| {
        captured
            .as_ref()
            .map(|captured| captured.handoff_scope(InstrumentationKind::Request))
    });
    instrument_future(handoff, async move {
        let overall: Option<BoxFuture<'a, ()>> = inherited_deadline.take().map(|mut deadline| {
            Box::pin(std::future::poll_fn(move |context| {
                deadline.as_mut().poll(context)
            })) as BoxFuture<'a, ()>
        });
        let input = match capture(
            state,
            transport,
            request,
            method,
            media,
            profile,
            authorized_init,
            deadlines,
            overall,
        ) {
            Ok(input) => input,
            Err(response) => return *response,
        };
        match input.prepare(captured.as_ref()) {
            Ok(prepared) => prepared.initialize().await,
            Err(response) => *response,
        }
    })
}

pub(super) fn response_only<C>(response: GraphQlResponse) -> SubscriptionInitialization<C> {
    SubscriptionInitialization::Response {
        response,
        headers: ResponseHeaders::new(),
        completion: SubscriptionCompletion::empty(),
    }
}
