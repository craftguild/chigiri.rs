//! Transport-neutral HTTP execution without exposing an application-level constructor.

pub(crate) mod deadline;
mod execution;
mod factory;
mod ingress;
pub(crate) mod observation;
mod pipeline;
pub(crate) mod response;
pub(crate) mod source;
pub(crate) mod streaming;
pub(crate) mod websocket;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_http::{
    BodyReadError, BoxFuture, DeadlineFactory, HttpLifecycle, RequestBody, ResponseParts,
    TransportRequest,
};
use chigiri_runtime::ExecutableSchema;

#[cfg(feature = "subscription-worker")]
pub(crate) use factory::ContextTaskScope;
pub(crate) use factory::{ErasedContextFactory, ErasedResponseHook, erase_factory, erase_hook};
pub(crate) use observation::Phase;

pub(crate) struct NeutralEndpoint<C> {
    state: Arc<EndpointState<C>>,
}

impl<C> Clone for NeutralEndpoint<C> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<C> NeutralEndpoint<C>
where
    C: Send + Sync + 'static,
{
    pub(crate) fn new(
        schema: ExecutableSchema<C>,
        lifecycle: HttpLifecycle,
        context_factory: Arc<dyn ErasedContextFactory<C>>,
        response_hook: Option<Arc<dyn ErasedResponseHook>>,
        settings: TransportSettings,
        subscriptions: crate::subscription::state::SubscriptionState,
    ) -> Self {
        let subscription_execution = Arc::new(
            crate::subscription::execution_state::SubscriptionExecutionState {
                #[cfg(feature = "subscription-worker")]
                managed: std::sync::OnceLock::new(),
                schema: schema.clone(),
                context_factory: Arc::clone(&context_factory),
                response_hook: response_hook.clone(),
                settings,
                subscriptions: subscriptions.settings,
            },
        );
        Self {
            state: Arc::new(EndpointState {
                schema,
                lifecycle,
                context_factory,
                response_hook,
                settings,
                subscriptions,
                subscription_execution,
                in_flight: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }

    pub(crate) fn handle<'a, B>(
        &'a self,
        request: TransportRequest<'a>,
        body: B,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, ResponseParts>
    where
        B: RequestBody + 'a,
    {
        use chigiri_runtime::instrumentation::{InstrumentationKind, instrument_future};

        let state = Arc::clone(&self.state);
        let captured = request
            .__instrumentation_capture()
            .cloned()
            .or_else(|| state.schema.capture_instrumentation(&request));
        let Some(captured) = captured else {
            return Box::pin(async move {
                let _request = InFlightRequest::start(Arc::clone(&state.in_flight));
                let telemetry = state.schema.request_telemetry(request.trace_id().cloned());
                pipeline::handle(state, request, body, deadlines, telemetry).await
            });
        };
        let handoff = captured.handoff_scope(InstrumentationKind::Request);
        instrument_future(Some(handoff), async move {
            let _request = InFlightRequest::start(Arc::clone(&state.in_flight));
            let telemetry = state.schema.request_telemetry_with_capture(
                request.trace_id().cloned(),
                Some(&captured),
                InstrumentationKind::Request,
            );
            let scope = telemetry.instrumentation_scope().cloned();
            instrument_future(scope.clone(), async move {
                let response = pipeline::handle(state, request, body, deadlines, telemetry).await;
                if let Some(scope) = scope {
                    scope.finish(observation::status_result(response.status).0);
                }
                response
            })
            .await
        })
    }

    pub(crate) fn in_flight_requests(&self) -> usize {
        self.state.in_flight.load(Ordering::Acquire)
    }

    pub(crate) fn shared_state(&self) -> Arc<EndpointState<C>> {
        Arc::clone(&self.state)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TransportSettings {
    pub(crate) body_limit: usize,
    pub(crate) response_byte_limit: usize,
    pub(crate) body_read_timeout: Duration,
    pub(crate) transport_admission_timeout: Duration,
    pub(crate) source_resolution_timeout: Duration,
    pub(crate) decoded_request_timeout: Duration,
    pub(crate) context_timeout: Duration,
    pub(crate) execution_timeout: Duration,
    pub(crate) response_hook_timeout: Duration,
    pub(crate) request_timeout: Duration,
}

pub(crate) struct EndpointState<C> {
    pub(crate) schema: ExecutableSchema<C>,
    pub(crate) lifecycle: HttpLifecycle,
    pub(crate) context_factory: Arc<dyn ErasedContextFactory<C>>,
    pub(crate) response_hook: Option<Arc<dyn ErasedResponseHook>>,
    pub(crate) settings: TransportSettings,
    pub(crate) subscriptions: crate::subscription::state::SubscriptionState,
    pub(crate) subscription_execution:
        Arc<crate::subscription::execution_state::SubscriptionExecutionState<C>>,
    pub(crate) in_flight: Arc<AtomicUsize>,
}

enum CollectError {
    TooLarge,
    Read(BodyReadError),
}

async fn collect<B: RequestBody>(body: &mut B, limit: usize) -> Result<Vec<u8>, CollectError> {
    let mut bytes = Vec::with_capacity(limit.min(8 * 1024));
    while let Some(chunk) = body.next_chunk().await.map_err(CollectError::Read)? {
        let chunk = chunk.as_ref();
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(CollectError::TooLarge);
        }
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}

#[derive(Clone, Copy)]
pub(crate) enum TimeoutKind {
    Phase,
    Overall,
}

pub(crate) async fn run<F, T>(
    deadlines: &dyn DeadlineFactory,
    phase_timeout: Duration,
    mut overall: std::pin::Pin<&mut (dyn std::future::Future<Output = ()> + Send)>,
    future: F,
) -> Result<T, TimeoutKind>
where
    F: std::future::Future<Output = T> + Send,
{
    use std::future::poll_fn;
    use std::task::Poll;

    let mut phase = deadlines.sleep(phase_timeout);
    let mut target = Box::pin(future);
    poll_fn(move |context| {
        if phase.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Phase));
        }
        if overall.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Overall));
        }
        target.as_mut().poll(context).map(Ok)
    })
    .await
}

struct InFlightRequest(Arc<AtomicUsize>);

impl InFlightRequest {
    fn start(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self(counter)
    }
}

impl Drop for InFlightRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
