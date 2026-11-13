//! Shared multi-slot and manual-clock supervision fixture wiring.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use chigiri_http::{
    BoxFuture, HookError, ResponseHeaders, ResponseHook, ResponseHookContext, SubscriptionTransport,
};

use super::super::super::state::{AppContext, HookState, State, count};
use super::super::support as worker;
use super::support::{configured_endpoint_for, pending_with_deadlines};
use crate::subscription::worker::SubscriptionRuntimeGuard;
use crate::transport::EndpointState;
use crate::transport::streaming::PreparedHttpSubscription;
use crate::transport::streaming::supervisor::{
    AcceptedHttpStreamingFrame, HttpStreamingBody, HttpStreamingDriver, HttpStreamingError,
    HttpStreamingFrame, HttpStreamingRegistration,
};

pub(super) struct AggregateSupervision {
    pub(super) endpoint: Arc<EndpointState<AppContext>>,
    pub(super) registration: HttpStreamingRegistration,
    pub(super) driver: Pin<Box<HttpStreamingDriver>>,
    pub(super) bodies: Vec<HttpStreamingBody>,
    pub(super) guard: SubscriptionRuntimeGuard,
}

pub(super) fn attach_aggregate(
    runtime: &tokio::runtime::Runtime,
    probe: Arc<State>,
    profiles: &[SubscriptionTransport],
    heartbeat: Duration,
    real_deadlines: bool,
) -> (AggregateSupervision, Arc<worker::Signals>) {
    let (mut endpoint, signals) =
        configured_endpoint_for(Arc::clone(&probe), profiles.len(), heartbeat);
    install_repeated_hook(&mut endpoint, probe);
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let (registration, mut driver) = HttpStreamingRegistration::new(bytes, profiles.len()).unwrap();
    driver.set_clock_for_test(0).unwrap();
    let bodies = profiles
        .iter()
        .copied()
        .map(|profile| {
            let pending = if real_deadlines {
                worker::run(
                    runtime,
                    pending_with_deadlines(Arc::clone(&endpoint), profile),
                )
            } else {
                worker::run(
                    runtime,
                    worker::pending(Arc::clone(&endpoint), profile, worker::VALID, false),
                )
            };
            let ready = match worker::run(runtime, pending.prepare()) {
                PreparedHttpSubscription::Ready(ready) => ready,
                PreparedHttpSubscription::Response(response) => {
                    panic!("real source setup returned {response:?}")
                }
                PreparedHttpSubscription::Failed(error) => {
                    panic!("real source setup failed: {error}")
                }
            };
            registration.attach(ready).unwrap().into_body()
        })
        .collect();
    (
        AggregateSupervision {
            endpoint,
            registration,
            driver: Box::pin(driver),
            bodies,
            guard,
        },
        signals,
    )
}

struct RepeatedHook(Arc<State>);

impl ResponseHook<HookState> for RepeatedHook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        let call = self
            .0
            .counts
            .hook_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        assert_eq!(count(&self.0.counts.setup_context_drops), call);
        Box::pin(async {
            let mut headers = ResponseHeaders::new();
            headers.append("x-subscription", "ready").unwrap();
            Ok(headers)
        })
    }
}

fn install_repeated_hook(endpoint: &mut Arc<EndpointState<AppContext>>, probe: Arc<State>) {
    let endpoint = Arc::get_mut(endpoint).unwrap();
    let hook = crate::transport::erase_hook(RepeatedHook(probe));
    endpoint.response_hook = Some(Arc::clone(&hook));
    Arc::get_mut(&mut endpoint.subscription_execution)
        .unwrap()
        .response_hook = Some(hook);
}

pub(super) fn set_time(live: &mut AggregateSupervision, nanos: u64) {
    live.driver
        .as_mut()
        .get_mut()
        .set_clock_for_test(nanos)
        .unwrap();
}

pub(super) fn poll_driver_once(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
) -> Poll<Result<(), HttpStreamingError>> {
    let _entered = runtime.enter();
    driver
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

pub(super) fn next_frame(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    body: &mut HttpStreamingBody,
) -> Result<Option<HttpStreamingFrame>, HttpStreamingError> {
    worker::run(
        runtime,
        poll_fn(|context| {
            match driver.as_mut().poll(context) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!("aggregate drained while the body was open"),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            }
            match body.poll_next(context) {
                Poll::Ready(Some(Ok(frame))) => Poll::Ready(Ok(Some(frame))),
                Poll::Ready(Some(Err(error))) => Poll::Ready(Err(error)),
                Poll::Ready(None) => Poll::Ready(Ok(None)),
                Poll::Pending => Poll::Pending,
            }
        }),
    )
}

pub(super) fn next_accepted(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    body: &mut HttpStreamingBody,
) -> Option<AcceptedHttpStreamingFrame> {
    next_frame(runtime, driver, body)
        .unwrap()
        .map(|frame| frame.accept().unwrap())
}
