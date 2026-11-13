use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader,
    SubscriptionTransport, TransportRequest,
};

use super::super::super::state::{AppContext, State};
use super::super::support as worker;
use crate::subscription::settings::ValidatedSubscriptionSettings;
use crate::subscription::state::SubscriptionState;
use crate::subscription::worker::SubscriptionRuntimeGuard;
use crate::transport::EndpointState;
use crate::transport::streaming::supervisor::{
    AcceptedHttpStreamingFrame, HttpStreamingBody, HttpStreamingDriver, HttpStreamingError,
    HttpStreamingFrame, HttpStreamingRegistration,
};
use crate::transport::streaming::{
    PendingHttpSubscription, PreparedHttpSubscription, SubscriptionHttpIngress,
};

pub(super) struct LiveSupervision {
    pub(super) endpoint: Arc<EndpointState<AppContext>>,
    pub(super) registration: HttpStreamingRegistration,
    pub(super) driver: Pin<Box<HttpStreamingDriver>>,
    pub(super) body: HttpStreamingBody,
    pub(super) guard: SubscriptionRuntimeGuard,
}

pub(super) fn attach(
    runtime: &tokio::runtime::Runtime,
    probe: Arc<State>,
    profile: SubscriptionTransport,
) -> (LiveSupervision, Arc<worker::Signals>) {
    attach_inner(runtime, probe, profile, false, false, false)
}

pub(super) fn attach_with_real_deadlines(
    runtime: &tokio::runtime::Runtime,
    probe: Arc<State>,
    profile: SubscriptionTransport,
) -> (LiveSupervision, Arc<worker::Signals>) {
    attach_inner(runtime, probe, profile, true, false, false)
}

pub(super) fn attach_with_sequenced_event_factory(
    runtime: &tokio::runtime::Runtime,
    probe: Arc<State>,
    profile: SubscriptionTransport,
    manual_delivery_clock: bool,
) -> (LiveSupervision, Arc<worker::Signals>) {
    attach_inner(runtime, probe, profile, true, true, manual_delivery_clock)
}

fn attach_inner(
    runtime: &tokio::runtime::Runtime,
    probe: Arc<State>,
    profile: SubscriptionTransport,
    real_deadlines: bool,
    sequenced_event_factory: bool,
    manual_delivery_clock: bool,
) -> (LiveSupervision, Arc<worker::Signals>) {
    let (mut endpoint, signals) = configured_endpoint(Arc::clone(&probe));
    if sequenced_event_factory {
        super::sequenced_factory::install(&mut endpoint, probe);
    }
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
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
        PreparedHttpSubscription::Response(response) => panic!(
            "real source setup returned {response:?}: {}",
            std::str::from_utf8(response.as_ref()).unwrap()
        ),
        PreparedHttpSubscription::Failed(error) => panic!("real source setup failed: {error}"),
    };
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let (registration, mut driver) = HttpStreamingRegistration::new(bytes, 1).unwrap();
    if manual_delivery_clock {
        driver.set_clock_for_test(0).unwrap();
    }
    let response = registration.attach(ready).unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .iter()
            .any(|header| header.name == "x-subscription" && header.value == "ready")
    );
    (
        LiveSupervision {
            endpoint,
            registration,
            driver: Box::pin(driver),
            body: response.into_body(),
            guard,
        },
        signals,
    )
}

fn configured_endpoint(
    probe: Arc<State>,
) -> (Arc<EndpointState<AppContext>>, Arc<worker::Signals>) {
    configured_endpoint_for(probe, 1, Duration::from_secs(60))
}

pub(super) fn configured_endpoint_for(
    probe: Arc<State>,
    maximum: usize,
    heartbeat: Duration,
) -> (Arc<EndpointState<AppContext>>, Arc<worker::Signals>) {
    let (mut endpoint, signals) = worker::endpoint(probe);
    let state = Arc::get_mut(&mut endpoint).unwrap();
    let mut limits = *state.subscriptions.settings.limits();
    limits.max_queued_events = 2;
    limits.max_active_subscriptions = maximum;
    limits.max_pending_operation_setups = maximum;
    limits.setup_timeout = Duration::from_secs(1);
    limits.event_timeout = Duration::from_millis(30);
    limits.send_no_progress_timeout = Duration::from_millis(100);
    limits.heartbeat_interval = heartbeat;
    let settings =
        ValidatedSubscriptionSettings::new(limits, *state.subscriptions.settings.protocol());
    state.subscriptions =
        SubscriptionState::new(settings, Arc::clone(&state.subscriptions.authorizer)).unwrap();
    state.settings.context_timeout = Duration::from_secs(1);
    state.settings.execution_timeout = Duration::from_secs(1);
    state.settings.response_hook_timeout = Duration::from_secs(1);
    state.settings.request_timeout = Duration::from_secs(1);
    let execution = Arc::get_mut(&mut state.subscription_execution).unwrap();
    execution.subscriptions = settings;
    execution.settings = state.settings;
    (endpoint, signals)
}

struct TokioDeadlines;

impl DeadlineFactory for TokioDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

static REAL_DEADLINES: TokioDeadlines = TokioDeadlines;

pub(super) async fn pending_with_deadlines(
    endpoint: Arc<EndpointState<AppContext>>,
    profile: SubscriptionTransport,
) -> PendingHttpSubscription<'static, AppContext> {
    let accept = match profile {
        SubscriptionTransport::ServerSentEvents => "text/event-stream",
        SubscriptionTransport::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        SubscriptionTransport::WebSocket => unreachable!(),
    };
    let metadata = ApplicationMetadata::default();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept.as_bytes()),
        RequestHeader::new("authorization", b"original-secret"),
    ];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    match crate::transport::streaming::handle(
        endpoint,
        transport,
        BufferedBody::new(worker::VALID.to_vec()),
        &REAL_DEADLINES,
    )
    .await
    {
        SubscriptionHttpIngress::Pending(pending) => *pending,
        SubscriptionHttpIngress::Finite(response) => panic!("unexpected finite {response:?}"),
    }
}

pub(super) fn drive_until(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    mut condition: impl FnMut() -> bool,
) {
    drive_until_observed(runtime, driver, |_| condition());
}

pub(super) fn drive_until_sources_pending(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    probe: &State,
    expected_instances: usize,
) {
    struct SourceObserver<'a>(&'a State, usize);
    impl Drop for SourceObserver<'_> {
        fn drop(&mut self) {
            self.0.observe_source(None);
            if std::thread::panicking() {
                eprintln!(
                    "source readiness: expected_instances={}, actual_instances={}, polls={}, {}",
                    self.1,
                    self.0
                        .source_pending_instances
                        .load(std::sync::atomic::Ordering::SeqCst),
                    self.0
                        .counts
                        .source_polls
                        .load(std::sync::atomic::Ordering::SeqCst),
                    crate::subscription::worker::process_diagnostic_for_test()
                );
            }
        }
    }
    let _observer = SourceObserver(probe, expected_instances);
    drive_until_observed(runtime, driver, |context| {
        probe.observe_source(Some(context.waker()));
        probe
            .source_pending_instances
            .load(std::sync::atomic::Ordering::SeqCst)
            == expected_instances
    });
}

pub(super) fn drive_until_observed(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    mut condition: impl FnMut(&Context<'_>) -> bool,
) {
    worker::run(
        runtime,
        poll_fn(|context| {
            match driver.as_mut().poll(context) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!("driver drained before registration close"),
                Poll::Ready(Err(error)) => panic!("driver failed before wire output: {error}"),
            }
            if condition(context) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    );
}

pub(super) fn next_frame(
    runtime: &tokio::runtime::Runtime,
    live: &mut LiveSupervision,
) -> Option<AcceptedHttpStreamingFrame> {
    next_unaccepted_frame(runtime, live)
        .unwrap_or_else(|error| panic!("supervised body failed instead of wire output: {error}"))
        .map(|frame| frame.accept().expect("whole frame acceptance"))
}

pub(super) fn next_unaccepted_frame(
    runtime: &tokio::runtime::Runtime,
    live: &mut LiveSupervision,
) -> Result<Option<HttpStreamingFrame>, HttpStreamingError> {
    let driver = &mut live.driver;
    let body = &mut live.body;
    worker::run(
        runtime,
        poll_fn(|context| {
            match driver.as_mut().poll(context) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!("driver drained while the body was open"),
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

pub(super) fn drive_for(
    runtime: &tokio::runtime::Runtime,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    duration: Duration,
) {
    worker::run(runtime, async {
        let timer = tokio::time::sleep(duration);
        tokio::pin!(timer);
        poll_fn(|context| {
            match driver.as_mut().poll(context) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!("driver drained before registration close"),
                Poll::Ready(Err(error)) => panic!("driver failed before wire output: {error}"),
            }
            if timer.as_mut().poll(context).is_ready() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    });
}

pub(super) fn close(runtime: &tokio::runtime::Runtime, mut live: LiveSupervision) {
    live.registration.close();
    worker::run(runtime, live.driver.as_mut()).unwrap();
    drop(live.body);
    worker::drain(runtime, live.guard);
}

pub(super) fn active(live: &LiveSupervision) -> usize {
    active_endpoint(&live.endpoint)
}

pub(super) fn active_endpoint(endpoint: &Arc<EndpointState<AppContext>>) -> usize {
    endpoint
        .subscriptions
        .resources
        .delivery()
        .snapshot()
        .endpoint_active_subscriptions
}

pub(super) fn text(frame: &AcceptedHttpStreamingFrame) -> &str {
    std::str::from_utf8(frame.as_ref()).unwrap()
}

pub(super) fn assert_complete(profile: SubscriptionTransport, frame: &[u8]) {
    match profile {
        SubscriptionTransport::ServerSentEvents => {
            assert_eq!(frame, b"event: complete\ndata: \n\n")
        }
        SubscriptionTransport::Multipart => assert_eq!(frame, b"--graphql--"),
        SubscriptionTransport::WebSocket => unreachable!(),
    }
}

pub(super) fn assert_safe_error(profile: SubscriptionTransport, frame: &[u8], code: &str) {
    let text = std::str::from_utf8(frame).unwrap();
    assert!(text.contains(code));
    assert!(!text.contains("source secret"));
    match profile {
        SubscriptionTransport::ServerSentEvents => {
            assert!(text.starts_with("event: next\ndata: "));
            assert!(text.contains("\"droppedTotal\":0"));
        }
        SubscriptionTransport::Multipart => {
            assert!(text.contains("\"payload\":null,\"errors\":"));
            assert!(!text.contains("\"locations\""));
            assert!(!text.contains("\"path\""));
        }
        SubscriptionTransport::WebSocket => unreachable!(),
    }
}

pub(super) fn poll_body_once(
    runtime: &tokio::runtime::Runtime,
    body: &mut HttpStreamingBody,
) -> Poll<
    Option<Result<crate::transport::streaming::supervisor::HttpStreamingFrame, HttpStreamingError>>,
> {
    let _entered = runtime.enter();
    let waker = std::task::Waker::noop();
    body.poll_next(&mut std::task::Context::from_waker(waker))
}
