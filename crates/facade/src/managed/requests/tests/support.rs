//! Small real finite executor and releasable request-context fixture.

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BufferedBody, DeadlineFactory, RequestHeader, ResponseCompletionOutcome,
    ResponseParts, TransportRequest,
};
use tokio::sync::watch;

use super::application::{Dispatch, Factory, Hook, SourceResolver};
use super::blocked::Block;
use crate::core::Source;
use crate::experimental::OwnedSchema;
use crate::managed::{ManagedHttpResponse, ManagedService};
use crate::{BoxFuture, Endpoint};

pub(super) struct Probe {
    pub(super) entered: AtomicUsize,
    pub(super) resolved: AtomicUsize,
    pub(super) hooks: AtomicUsize,
    pub(super) context_drops: AtomicUsize,
    pub(super) context_future_drops: AtomicUsize,
    pub(super) pending_source: std::sync::atomic::AtomicBool,
    pub(super) source_entered: AtomicUsize,
    pub(super) stop_at_source_return: Mutex<Option<Arc<crate::managed::Control>>>,
    pub(super) source_drops: AtomicUsize,
    pub(super) pending_execution: std::sync::atomic::AtomicBool,
    pub(super) pending_hook: std::sync::atomic::AtomicBool,
    pub(super) executing: AtomicUsize,
    pub(super) resolver_drops: AtomicUsize,
    pub(super) completion_drops: AtomicUsize,
    pub(super) completion_saw_disposed_resolver: std::sync::atomic::AtomicBool,
    pub(super) mode: Mutex<Mode>,
    pub(super) observer_panics: std::sync::atomic::AtomicBool,
    pub(super) errors: Mutex<Vec<(String, String)>>,
    pub(super) completions: Mutex<Vec<ResponseCompletionOutcome>>,
    pub(super) block_context: Mutex<Option<Arc<Block>>>,
    pub(super) block_completion: Mutex<Option<Arc<Block>>>,
    pub(super) release: watch::Sender<bool>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Mode {
    Normal,
    ResolverPanic,
    HookError,
    CompletionPanic,
    CompletionDropPanic,
    CompletionPayloadDropPanic,
    ContextReadyDropPanic,
}

impl Probe {
    pub(super) fn release(&self) {
        self.release.send_replace(true);
    }
}

pub(super) struct Harness {
    pub(super) endpoint: Endpoint<()>,
    pub(super) probe: Arc<Probe>,
}

impl Harness {
    pub(super) fn new(overall: Duration) -> Self {
        Self::with_phase_timeout(overall, Duration::from_secs(1))
    }

    pub(super) fn with_phase_timeout(overall: Duration, context: Duration) -> Self {
        let (release, _) = watch::channel(false);
        let probe = Arc::new(Probe {
            entered: AtomicUsize::new(0),
            resolved: AtomicUsize::new(0),
            hooks: AtomicUsize::new(0),
            context_drops: AtomicUsize::new(0),
            context_future_drops: AtomicUsize::new(0),
            pending_source: std::sync::atomic::AtomicBool::new(false),
            source_entered: AtomicUsize::new(0),
            stop_at_source_return: Mutex::new(None),
            source_drops: AtomicUsize::new(0),
            pending_execution: std::sync::atomic::AtomicBool::new(false),
            pending_hook: std::sync::atomic::AtomicBool::new(false),
            executing: AtomicUsize::new(0),
            resolver_drops: AtomicUsize::new(0),
            completion_drops: AtomicUsize::new(0),
            completion_saw_disposed_resolver: std::sync::atomic::AtomicBool::new(false),
            mode: Mutex::new(Mode::Normal),
            observer_panics: std::sync::atomic::AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
            completions: Mutex::new(Vec::new()),
            block_context: Mutex::new(None),
            block_completion: Mutex::new(None),
            release,
        });
        let schema = OwnedSchema::parse(vec![Source::new(
            "owned-http.graphql",
            "type Query { ping: String! } type Mutation { ping: String! }",
        )])
        .unwrap();
        let executable = schema
            .clone()
            .bind(Dispatch {
                schema: schema.clone(),
                probe: Arc::clone(&probe),
                coordinates: [
                    (Arc::from("Query"), Arc::from("ping")),
                    (Arc::from("Mutation"), Arc::from("ping")),
                ],
            })
            .unwrap();
        let endpoint = Endpoint::builder(executable)
            .context_factory(Factory(Arc::clone(&probe)))
            .response_hook(Hook(Arc::clone(&probe)))
            .operation_source(SourceResolver(Arc::clone(&probe)))
            .inject(|components| {
                components
                    .http()
                    .overall_timeout(overall)
                    .context_timeout(context)
                    .body_timeout(context)
                    .source_resolution_timeout(context)
                    .execution_timeout(context)
                    .response_hook_timeout(context);
                components
                    .observability()
                    .error_observer(super::application::Observer(Arc::clone(&probe)));
            })
            .build()
            .unwrap();
        Self { endpoint, probe }
    }
}

struct Deadlines;
impl DeadlineFactory for Deadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
static DEADLINES: Deadlines = Deadlines;

pub(super) fn deadlines() -> &'static dyn DeadlineFactory {
    &DEADLINES
}

pub(super) fn invoke(
    endpoint: &Endpoint<()>,
    service: &ManagedService,
    document: &str,
) -> BoxFuture<'static, ManagedHttpResponse> {
    invoke_body(
        endpoint,
        service,
        BufferedBody::new(format!("{{\"query\":\"{document}\"}}").into_bytes()),
    )
}

pub(super) fn invoke_body<B: chigiri_http::RequestBody + 'static>(
    endpoint: &Endpoint<()>,
    service: &ManagedService,
    body: B,
) -> BoxFuture<'static, ManagedHttpResponse> {
    let metadata = ApplicationMetadata::default();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    endpoint.__handle_managed_http(
        service.request().unwrap(),
        TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata),
        body,
        crate::__streaming::WebSocketHttpVersion::Http11,
        &DEADLINES,
    )
}

pub(super) fn finite(response: ManagedHttpResponse) -> ResponseParts {
    match response {
        ManagedHttpResponse::Finite(parts) => parts,
        _ => panic!("expected finite HTTP response"),
    }
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()
        .unwrap()
}

pub(super) async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded owned HTTP fixture");
}

pub(super) async fn joined(endpoint: &Endpoint<()>) {
    let control = endpoint.managed_control();
    until(|| !crate::subscription::worker::managed_supervisor_retained_for_test(&control)).await;
}
