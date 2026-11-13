use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, DeadlineFactory, RequestHeader, RequestMethod, ResponseMediaType,
    ResponseParts, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::{ExecutableSchema, GraphQlRequest, Object};

use super::super::SubscriptionInitialization;
use super::deadlines::{
    CONTEXT_TIMEOUT, EVENT_TIMEOUT, EXECUTION_TIMEOUT, HOOK_TIMEOUT, REQUEST_TIMEOUT, SETUP_TIMEOUT,
};
use super::descriptor::SCHEMA;
use super::dispatch::Dispatch;
use super::state::{AppContext, State};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::subscription::state::SubscriptionState;
use crate::transport::{
    EndpointState, NeutralEndpoint, TransportSettings, erase_factory, erase_hook,
};

mod application;
pub(super) use application::{Factory, Hook, RequestIds};

#[cfg(feature = "subscription-worker")]
pub(super) mod event_gate;
#[cfg(feature = "subscription-worker")]
pub(super) mod managed_socket;
#[cfg(feature = "subscription-worker")]
mod session_endpoint;
#[cfg(feature = "subscription-worker")]
pub(super) use session_endpoint::observed_session_endpoint;
#[cfg(feature = "subscription-worker")]
pub(super) use session_endpoint::session_endpoint;

pub(super) fn endpoint(probe: Arc<State>, metadata_limit: usize) -> Arc<EndpointState<AppContext>> {
    endpoint_with_lifecycle(probe, metadata_limit, chigiri_http::HttpLifecycle::new())
}

pub(super) fn endpoint_with_lifecycle(
    probe: Arc<State>,
    metadata_limit: usize,
    lifecycle: chigiri_http::HttpLifecycle,
) -> Arc<EndpointState<AppContext>> {
    let schema =
        ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(Arc::clone(&probe)))
            .request_id_factory(RequestIds(Arc::clone(&probe)))
            .build()
            .unwrap();
    NeutralEndpoint::new(
        schema,
        lifecycle,
        erase_factory(Factory(Arc::clone(&probe))),
        Some(erase_hook(Hook(probe))),
        TransportSettings {
            body_limit: 1024,
            response_byte_limit: 4096,
            body_read_timeout: Duration::from_secs(1),
            transport_admission_timeout: Duration::from_secs(1),
            source_resolution_timeout: Duration::from_secs(1),
            decoded_request_timeout: Duration::from_secs(1),
            context_timeout: CONTEXT_TIMEOUT,
            execution_timeout: EXECUTION_TIMEOUT,
            response_hook_timeout: HOOK_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
        },
        SubscriptionState::new(
            ValidatedSubscriptionSettings::new(
                SubscriptionLimits {
                    max_retained_metadata_bytes: metadata_limit,
                    setup_timeout: SETUP_TIMEOUT,
                    event_timeout: EVENT_TIMEOUT,
                    ..SubscriptionLimits::default()
                },
                SubscriptionProtocolSettings::default(),
            ),
            Arc::new(chigiri_http::AcceptConnectionInit),
        )
        .unwrap(),
    )
    .shared_state()
}

pub(super) async fn initialize(
    state: Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    profile: SubscriptionTransport,
    deadlines: &dyn DeadlineFactory,
) -> SubscriptionInitialization<AppContext> {
    initialize_inner(
        state,
        request,
        profile,
        deadlines,
        Option::<std::future::Pending<()>>::None,
    )
    .await
}

pub(super) async fn initialize_with_deadline(
    state: Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    profile: SubscriptionTransport,
    deadlines: &dyn DeadlineFactory,
    overall: impl Future<Output = ()> + Send,
) -> SubscriptionInitialization<AppContext> {
    initialize_inner(state, request, profile, deadlines, Some(overall)).await
}

async fn initialize_inner(
    state: Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    profile: SubscriptionTransport,
    deadlines: &dyn DeadlineFactory,
    overall: Option<impl Future<Output = ()> + Send>,
) -> SubscriptionInitialization<AppContext> {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"original-secret")];
    let transport = TransportRequest::new(
        "POST",
        "/graphql",
        Some("private=query"),
        &headers,
        None,
        &metadata,
    );
    let mut overall = overall.map(Box::pin);
    let overall = overall
        .as_mut()
        .map(|future| future.as_mut() as Pin<&mut (dyn Future<Output = ()> + Send)>);
    super::super::start(
        state,
        transport,
        request,
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        profile,
        None,
        deadlines,
        overall,
    )
    .await
}

pub(super) fn request() -> GraphQlRequest {
    let mut variables = Object::new();
    variables.insert("limit", 2_i32);
    GraphQlRequest::new(
        "subscription Named($limit: Int!) { chosen: events(limit: $limit) { value } }",
    )
    .with_operation_name("Named")
    .with_variables(variables)
}

pub(super) fn invalid_request() -> GraphQlRequest {
    GraphQlRequest::new("subscription Named { chosen: events(limit: 2) { missing } }")
        .with_operation_name("Named")
}

pub(super) fn query_request() -> GraphQlRequest {
    GraphQlRequest::new("query { ping }")
}

pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        match poll_once(future.as_mut()) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

pub(super) fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

pub(super) fn response_parts() -> ResponseParts {
    ResponseParts {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    }
}
