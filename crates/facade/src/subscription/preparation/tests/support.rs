use std::future::{Future, pending};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, RequestHeader, RequestMethod,
    ResponseMediaType, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::{LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult};
use chigiri_runtime::{RequestId, RequestIdFactory, RequestIdFactoryError};

use crate::subscription::request::test_support::{builder, graph_request};
use crate::subscription::request::{MetadataRetentionLimit, SubscriberRequest};

pub(super) struct NeverDeadline;

impl DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(pending())
    }
}

pub(super) struct ImmediateDeadline;

impl DeadlineFactory for ImmediateDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

pub(super) struct SecondPollDeadline;

impl DeadlineFactory for SecondPollDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        let mut polled = false;
        Box::pin(std::future::poll_fn(move |context| {
            if polled {
                Poll::Ready(())
            } else {
                polled = true;
                context.waker().wake_by_ref();
                Poll::Pending
            }
        }))
    }
}

pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    struct Signal(std::thread::Thread);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Signal(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

pub(super) fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
    struct Signal(std::thread::Thread);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Signal(std::thread::current())));
    future.poll(&mut Context::from_waker(&waker))
}

struct CountingRequestIds(Arc<AtomicUsize>);

impl RequestIdFactory for CountingRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let call = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        RequestId::new(format!("request-{call}"))
            .map_err(|error| RequestIdFactoryError::new(std::io::Error::other(error)))
    }
}

pub(super) fn request(
    profile: SubscriptionTransport,
) -> (chigiri_runtime::ExecutableSchema<()>, SubscriberRequest) {
    let schema = builder()
        .request_id_factory(CountingRequestIds(Arc::new(AtomicUsize::new(0))))
        .build()
        .unwrap();
    request_from_schema(schema, profile)
}

pub(super) type LifecycleRecords =
    Arc<Mutex<Vec<(LifecyclePhase, LifecycleResult, Option<&'static str>)>>>;

struct Observer(LifecycleRecords);

impl LifecycleObserver for Observer {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.code()));
    }
}

pub(super) fn observed_request(
    profile: SubscriptionTransport,
    records: LifecycleRecords,
) -> (chigiri_runtime::ExecutableSchema<()>, SubscriberRequest) {
    let schema = builder()
        .request_id_factory(CountingRequestIds(Arc::new(AtomicUsize::new(0))))
        .lifecycle_observer(Observer(records))
        .build()
        .unwrap();
    request_from_schema(schema, profile)
}

fn request_from_schema(
    schema: chigiri_runtime::ExecutableSchema<()>,
    profile: SubscriptionTransport,
) -> (chigiri_runtime::ExecutableSchema<()>, SubscriberRequest) {
    let prepared = schema.prepare_subscription(graph_request()).unwrap();
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
    let request = SubscriberRequest::retain(
        transport,
        &prepared,
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        profile,
        None,
        MetadataRetentionLimit::default(),
    )
    .unwrap();
    (schema, request)
}

pub(super) fn overall() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(pending())
}

pub(super) fn response() -> chigiri_runtime::GraphQlResponse {
    chigiri_runtime::GraphQlResponse {
        data: None,
        errors: Vec::new(),
        extensions: chigiri_runtime::Object::new(),
    }
}
