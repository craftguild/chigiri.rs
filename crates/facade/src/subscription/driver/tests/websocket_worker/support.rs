use std::future::{Future, pending};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, RequestHeader, RequestMethod,
    ResponseMediaType, TransportRequest,
};
use chigiri_runtime::{GraphQlRequest, Object};

use super::super::deadlines::SETUP_TIMEOUT;
use super::super::harness;
use super::super::state::{AppContext, State};
use crate::subscription::bridge_resources::{BridgeConnection, PendingBridgeOperation};
use crate::subscription::driver::input::WebSocketOperationValidation;
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::request::{MetadataRetentionLimit, SubscriberIngress};
use crate::subscription::worker::SubscriptionRuntimeGuard;
use crate::subscription::worker::websocket::{WebSocketSetupDeadline, WebSocketValidationWorker};
use crate::transport::EndpointState;

pub(in crate::subscription::driver::tests) struct NeverDeadlines;

impl DeadlineFactory for NeverDeadlines {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(pending())
    }
}

pub(in crate::subscription::driver::tests) static NEVER_DEADLINES: NeverDeadlines = NeverDeadlines;

#[derive(Default)]
pub(in crate::subscription::driver::tests) struct TokioDeadlines {
    starts: Mutex<Vec<Duration>>,
}

impl TokioDeadlines {
    pub(in crate::subscription::driver::tests) fn starts(&self) -> Vec<Duration> {
        self.starts.lock().unwrap().clone()
    }
}

impl DeadlineFactory for TokioDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.starts.lock().unwrap().push(duration);
        Box::pin(async move { tokio::time::sleep(duration).await })
    }
}

pub(in crate::subscription::driver::tests) struct Registered {
    pub(in crate::subscription::driver::tests) runtime: tokio::runtime::Runtime,
    pub(in crate::subscription::driver::tests) endpoint: Arc<EndpointState<AppContext>>,
    pub(in crate::subscription::driver::tests) guard: SubscriptionRuntimeGuard,
    pub(in crate::subscription::driver::tests) connection: BridgeConnection,
}

impl Registered {
    pub(in crate::subscription::driver::tests) fn new(probe: Arc<State>) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .unwrap();
        let endpoint = harness::endpoint(probe, 16 * 1024);
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let connection = endpoint.subscriptions.resources.open_connection().unwrap();
        Self {
            runtime,
            endpoint,
            guard,
            connection,
        }
    }

    pub(in crate::subscription::driver::tests) fn begin(
        &self,
        id: Arc<str>,
    ) -> PendingBridgeOperation {
        self.connection.begin(id).unwrap()
    }

    pub(in crate::subscription::driver::tests) fn drain(self) {
        drop(self.connection.revoke().unwrap());
        run(&self.runtime, self.guard.shutdown().unwrap()).unwrap();
    }
}

pub(in crate::subscription::driver::tests) fn worker(
    runtime: &tokio::runtime::Runtime,
    endpoint: &Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    init: Option<Object>,
    operation: PendingBridgeOperation,
    deadlines: &'static dyn DeadlineFactory,
) -> WebSocketValidationWorker<AppContext> {
    let validation = validation(endpoint, request, init);
    let deadline = setup_deadline(deadlines);
    let _entered = runtime.enter();
    let failure = SubscriptionFailureReporter::new(
        endpoint.subscription_execution.schema.clone(),
        endpoint.schema.request_telemetry(None),
        "subscription-websocket-worker-test",
    );
    WebSocketValidationWorker::new(validation, operation, deadline, failure)
        .unwrap_or_else(|_| panic!("WebSocket validation worker did not start"))
}

pub(in crate::subscription::driver::tests) fn validation(
    endpoint: &Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    init: Option<Object>,
) -> WebSocketOperationValidation<AppContext> {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"original-secret")];
    let transport = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    let ingress = SubscriberIngress::capture_websocket(
        transport,
        &request,
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        init.map(Arc::new),
        MetadataRetentionLimit::from_validated(&endpoint.subscriptions.settings),
    )
    .unwrap();
    WebSocketOperationValidation::new(
        Arc::clone(&endpoint.subscription_execution),
        request,
        ingress,
    )
}

pub(in crate::subscription::driver::tests) fn setup_deadline(
    deadlines: &'static dyn DeadlineFactory,
) -> WebSocketSetupDeadline {
    WebSocketSetupDeadline::new(deadlines, deadlines.sleep(SETUP_TIMEOUT))
}

pub(in crate::subscription::driver::tests) fn echo_request() -> GraphQlRequest {
    let mut variables = Object::new();
    variables.insert("value", "worker-value");
    GraphQlRequest::new("query Named($value: Probe!) { echo(value: $value) }")
        .with_operation_name("Named")
        .with_variables(variables)
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
            .unwrap_or_else(|_| panic!("bounded WebSocket worker fixture at {caller}"))
    })
}
