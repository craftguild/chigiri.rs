use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri::core::Source;
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
use chigiri::spi::{
    AdmissionDecision, AdmissionError, ApplicationMetadata, BodyReadError, DecodedRequestHook,
    DecodedRequestView, OperationSourceResolver, RequestBody, RequestEnvelope, RequestHeader,
    ResolvedOperationSource, SourceResolutionError, TransportAdmission, TransportRequest,
};
use chigiri::{
    BoxFuture, ContextFactory, Endpoint, FieldResult, HttpRequestContext, LifecycleEvent,
    LifecycleObserver, LifecyclePhase, LifecycleResult, PrepareError, PreparedContext,
    SubscriptionLimits, Value,
};

use chigiri::__streaming::SubscriptionHttpIngress;

pub use super::clock::{TrackingDeadlines, block_on};

pub const OVERALL: Duration = Duration::from_secs(23);
pub const SETUP: Duration = Duration::from_secs(11);

#[derive(Default)]
pub struct Signals {
    pub admission: AtomicUsize,
    pub source: AtomicUsize,
    pub decoded: AtomicUsize,
    pub context: AtomicUsize,
    pub scalar: AtomicUsize,
    pub resolver: AtomicUsize,
    pub deadline_visible: AtomicUsize,
    pub decoded_queries: Mutex<Vec<String>>,
    pub lifecycle: Mutex<Vec<(LifecyclePhase, LifecycleResult, Option<&'static str>)>>,
}

impl Signals {
    pub fn count(&self, phase: LifecyclePhase, result: LifecycleResult) -> usize {
        self.lifecycle
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.0 == phase && event.1 == result)
            .count()
    }

    pub fn count_code(
        &self,
        phase: LifecyclePhase,
        result: LifecycleResult,
        code: Option<&'static str>,
    ) -> usize {
        self.lifecycle
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == (phase, result, code))
            .count()
    }
}

struct Dispatch {
    schema: OwnedSchema,
    calls: Arc<Signals>,
    coordinates: [(Arc<str>, Arc<str>); 1],
}

impl OwnedResolverDispatch<()> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }

    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }

    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        let calls = Arc::clone(&self.calls);
        registry.register("Probe", move |_, value| {
            calls.scalar.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        });
    }

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.resolver.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(ResolvedValue::Leaf(
                arguments
                    .into_values()
                    .next()
                    .and_then(|value| value.into_value())
                    .unwrap_or(Value::Int(7)),
            ))
        })
    }
}

struct Factory(Arc<Signals>);

impl ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        _: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        self.0.context.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(PreparedContext::execution_only(())) })
    }
}

struct Admission(Arc<Signals>);

impl TransportAdmission for Admission {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.admission.fetch_add(1, Ordering::SeqCst);
        if request.deadline_factory().is_some() {
            self.0.deadline_visible.fetch_add(1, Ordering::SeqCst);
        }
        Box::pin(async { Ok(AdmissionDecision::Continue) })
    }
}

struct SourceResolver {
    calls: Arc<Signals>,
    persisted_document: Arc<str>,
}

impl OperationSourceResolver for SourceResolver {
    fn resolve<'a>(
        &'a self,
        envelope: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        self.calls.source.fetch_add(1, Ordering::SeqCst);
        let document = envelope
            .document_text()
            .map(str::to_owned)
            .unwrap_or_else(|| self.persisted_document.to_string());
        Box::pin(async move { Ok(ResolvedOperationSource::new(document)) })
    }
}

struct Decoded(Arc<Signals>);

impl DecodedRequestHook for Decoded {
    fn inspect<'a>(
        &'a self,
        request: DecodedRequestView<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.decoded.fetch_add(1, Ordering::SeqCst);
        self.0
            .decoded_queries
            .lock()
            .unwrap()
            .push(request.request().query.clone());
        Box::pin(async { Ok(AdmissionDecision::Continue) })
    }
}

struct Observer(Arc<Signals>);

impl LifecycleObserver for Observer {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .lifecycle
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.code()));
    }
}

pub struct Harness {
    pub endpoint: Endpoint<()>,
    pub signals: Arc<Signals>,
}

pub fn harness(metadata_limit: usize, pending_limit: usize, persisted: &str) -> Harness {
    let schema = OwnedSchema::parse(vec![Source::new(
        "streaming-ingress.graphql",
        "scalar Probe type Query { echo(value: Probe): Probe }",
    )])
    .unwrap();
    let signals = Arc::new(Signals::default());
    let dispatch = Dispatch {
        schema: schema.clone(),
        calls: Arc::clone(&signals),
        coordinates: [(Arc::from("Query"), Arc::from("echo"))],
    };
    let limits = SubscriptionLimits {
        max_pending_operation_setups: pending_limit,
        max_retained_metadata_bytes: metadata_limit,
        setup_timeout: SETUP,
        ..SubscriptionLimits::default()
    };
    let endpoint = Endpoint::builder(schema.bind(dispatch).unwrap())
        .context_factory(Factory(Arc::clone(&signals)))
        .operation_source(SourceResolver {
            calls: Arc::clone(&signals),
            persisted_document: Arc::from(persisted),
        })
        .subscription_limits(limits)
        .inject(|components| {
            let mut http = components.http();
            http.push_transport_admission(Admission(Arc::clone(&signals)))
                .push_decoded_hook(Decoded(Arc::clone(&signals)))
                .transport_admission_timeout(Duration::from_secs(1))
                .body_timeout(Duration::from_secs(2))
                .source_resolution_timeout(Duration::from_secs(3))
                .decoded_request_timeout(Duration::from_secs(4))
                .context_timeout(Duration::from_secs(5))
                .execution_timeout(Duration::from_secs(6))
                .response_hook_timeout(Duration::from_secs(7))
                .overall_timeout(OVERALL);
            components
                .observability()
                .lifecycle_observer(Observer(Arc::clone(&signals)));
        })
        .build()
        .unwrap();
    signals.lifecycle.lock().unwrap().clear();
    Harness { endpoint, signals }
}

pub struct RequestInput<'a> {
    pub method: &'a str,
    pub uri_query: Option<&'a str>,
    pub headers: &'a [RequestHeader<'a>],
    pub body: Vec<u8>,
}

pub fn invoke<'deadline>(
    endpoint: &Endpoint<()>,
    deadlines: &'deadline TrackingDeadlines,
    input: RequestInput<'_>,
) -> (SubscriptionHttpIngress<'deadline, ()>, Arc<AtomicUsize>) {
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new(
        input.method,
        "/graphql",
        input.uri_query,
        input.headers,
        None,
        &metadata,
    );
    let polls = Arc::new(AtomicUsize::new(0));
    let body = CountingBody {
        chunk: Some(input.body),
        polls: Arc::clone(&polls),
    };
    (
        block_on(endpoint.__handle_http_streaming(transport, body, deadlines)),
        polls,
    )
}

pub fn json_headers(accept: &[u8]) -> [RequestHeader<'_>; 2] {
    [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept),
    ]
}

struct CountingBody {
    chunk: Option<Vec<u8>>,
    polls: Arc<AtomicUsize>,
}

impl RequestBody for CountingBody {
    type Chunk = Vec<u8>;

    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let chunk = self.chunk.take();
        Box::pin(async move { Ok(chunk) })
    }
}
