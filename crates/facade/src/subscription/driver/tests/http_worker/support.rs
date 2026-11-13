use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BufferedBody, RequestHeader, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::{LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult};

use super::super::deadlines::Deadlines;
use super::super::harness;
use super::super::state::{AppContext, State};
use crate::subscription::settings::{SubscriptionLimits, ValidatedSubscriptionSettings};
use crate::subscription::state::SubscriptionState;
use crate::subscription::worker::SubscriptionRuntimeGuard;
use crate::transport::EndpointState;
use crate::transport::streaming::{PendingHttpSubscription, SubscriptionHttpIngress};

pub(in crate::subscription::driver::tests) const VALID: &[u8] = br#"{"query":"subscription Named($limit: Int!) { chosen: events(limit: $limit) { value } }","operationName":"Named","variables":{"limit":2}}"#;
pub(in crate::subscription::driver::tests) const INVALID: &[u8] = br#"{"query":"subscription Named { chosen: events(limit: 2) { missing } }","operationName":"Named"}"#;
static DEADLINES: Deadlines = Deadlines(Vec::new());

type Event = (LifecyclePhase, LifecycleResult, Option<&'static str>);

#[derive(Default)]
pub(in crate::subscription::driver::tests) struct Signals(Mutex<Vec<Event>>);

struct Observer(Arc<Signals>);

impl LifecycleObserver for Observer {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .0
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.code()));
    }
}

impl Signals {
    pub(in crate::subscription::driver::tests) fn attach(
        self: &Arc<Self>,
        endpoint: &mut Arc<EndpointState<AppContext>>,
    ) {
        let state = Arc::get_mut(endpoint).expect("one fixture-owned endpoint");
        state.schema = state
            .schema
            .reconfigure()
            .lifecycle_observer(Observer(Arc::clone(self)))
            .build();
        Arc::get_mut(&mut state.subscription_execution)
            .expect("one fixture-owned execution graph")
            .schema = state.schema.clone();
    }

    pub(in crate::subscription::driver::tests) fn completed(
        &self,
    ) -> Vec<(LifecycleResult, Option<&'static str>)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(phase, result, code)| {
                (*phase == LifecyclePhase::RequestCompleted).then_some((*result, *code))
            })
            .collect()
    }
}

pub(in crate::subscription::driver::tests) fn endpoint(
    probe: Arc<State>,
) -> (Arc<EndpointState<AppContext>>, Arc<Signals>) {
    let mut endpoint = harness::endpoint(probe, 16 * 1024);
    let signals = Arc::new(Signals::default());
    let state = Arc::get_mut(&mut endpoint).expect("one test-owned endpoint");
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits {
            max_active_subscriptions: 1,
            max_pending_operation_setups: 1,
            ..*state.subscriptions.settings.limits()
        },
        *state.subscriptions.settings.protocol(),
    );
    state.subscriptions =
        SubscriptionState::new(settings, Arc::clone(&state.subscriptions.authorizer)).unwrap();
    state.schema = state
        .schema
        .reconfigure()
        .lifecycle_observer(Observer(Arc::clone(&signals)))
        .build();
    let execution =
        Arc::get_mut(&mut state.subscription_execution).expect("one test-owned execution graph");
    execution.schema = state.schema.clone();
    execution.subscriptions = settings;
    (endpoint, signals)
}

pub(in crate::subscription::driver::tests) async fn pending(
    endpoint: Arc<EndpointState<AppContext>>,
    profile: SubscriptionTransport,
    body: &[u8],
    finite_json: bool,
) -> PendingHttpSubscription<'static, AppContext> {
    let accept = match (profile, finite_json) {
        (SubscriptionTransport::ServerSentEvents, false) => "text/event-stream",
        (SubscriptionTransport::ServerSentEvents, true) => "text/event-stream, application/json",
        (SubscriptionTransport::Multipart, false) => "multipart/mixed; subscriptionSpec=1.0",
        (SubscriptionTransport::Multipart, true) => {
            "multipart/mixed; subscriptionSpec=1.0, application/json"
        }
        (SubscriptionTransport::WebSocket, _) => unreachable!(),
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
        BufferedBody::new(body.to_vec()),
        &DEADLINES,
    )
    .await
    {
        SubscriptionHttpIngress::Pending(pending) => *pending,
        SubscriptionHttpIngress::Finite(response) => {
            panic!("expected admitted subscription, got {response:?}")
        }
    }
}

pub(in crate::subscription::driver::tests) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap()
}

#[track_caller]
pub(in crate::subscription::driver::tests) fn run<F: Future>(
    runtime: &tokio::runtime::Runtime,
    future: F,
) -> F::Output {
    let caller = std::panic::Location::caller();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), future)
            .await
            .unwrap_or_else(|_| panic!("bounded HTTP worker fixture at {caller}"))
    })
}

pub(in crate::subscription::driver::tests) fn drain(
    runtime: &tokio::runtime::Runtime,
    guard: SubscriptionRuntimeGuard,
) {
    let mut shutdown = Box::pin(guard.shutdown().unwrap());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(runtime, shutdown.as_mut())
    }));
    match result {
        Ok(result) => result.unwrap(),
        Err(payload) => {
            eprintln!(
                "worker drain retained states [idle, running, retired, polling, poisoned, failed, released]: {:?}",
                shutdown.retained_states_for_test()
            );
            std::panic::resume_unwind(payload);
        }
    }
}

pub(in crate::subscription::driver::tests) fn profiles() -> [SubscriptionTransport; 2] {
    [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ]
}
