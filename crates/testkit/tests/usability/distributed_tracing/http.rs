//! Shared finite HTTP entry adopts headers before any future handoff.

use std::future::poll_fn;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use chigiri::spi::{ApplicationMetadata, RequestHeader};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::super::support::{NeverDeadline, block_on};
use super::{FirstIdentityOnly, Observed, PrivateValue, parent, query, recording};

pub(super) type Seen = Arc<Mutex<Vec<(SpanContext, bool)>>>;
pub(super) struct Factory(pub(super) Seen);

impl Factory {
    fn observe(&self, request: chigiri_http::RequestContext<'_>) {
        let current = Context::current();
        let span = current.span().span_context().clone();
        assert!(current.get::<PrivateValue>().is_none());
        assert_eq!(
            request
                .transport()
                .request_telemetry()
                .unwrap()
                .trace_id()
                .unwrap()
                .as_str(),
            span.trace_id().to_string()
        );
        self.0
            .lock()
            .unwrap()
            .push((span, current.is_telemetry_suppressed()));
    }
}

impl chigiri_http::ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        request: chigiri_http::RequestContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::PreparedContext<(), ()>, chigiri::PrepareError>>
    {
        self.observe(request);
        Box::pin(async move {
            self.observe(request);
            let mut pending = true;
            poll_fn(|task| {
                if pending {
                    pending = false;
                    task.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            self.observe(request);
            Ok(chigiri::PreparedContext::execution_only(()))
        })
    }
}

#[test]
fn finite_http_context_factory_and_resolver_adopt_headers_or_current_parent_before_handoff() {
    for suppressed in [false, true] {
        for current_parent in [false, true] {
            let (_provider, tracer, records) = recording::provider();
            let calls = Arc::clone(&tracer.calls);
            let observed = Observed::default();
            let seen = Seen::default();
            let endpoint = chigiri::Endpoint::builder(query::resolvers())
                .inject(|components| {
                    components
                        .observability()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                        .request_id_factory(FirstIdentityOnly(std::sync::atomic::AtomicUsize::new(
                            0,
                        )))
                        .lifecycle_observer(observed.clone())
                        .error_observer(observed.clone());
                })
                .context_factory(Factory(Arc::clone(&seen)))
                .build()
                .unwrap();
            let headers = [
                RequestHeader::new("accept", b"application/graphql-response+json"),
                RequestHeader::new("x-chigiri-preflight", b"1"),
                RequestHeader::new(
                    "traceparent",
                    b"00-08080808080808080808080808080808-0808080808080808-01",
                ),
                RequestHeader::new("tracestate", b"vendor=value"),
                RequestHeader::new("authorization", b"private-header"),
                RequestHeader::new("baggage", b"private-baggage"),
            ];
            let metadata = ApplicationMetadata::default();
            let opaque = chigiri::TraceId::new("opaque-transport").unwrap();
            let request = chigiri::TransportRequest::new(
                "GET",
                "/graphql",
                Some("query=%7B%20greeting%20%7D"),
                &headers,
                None,
                &metadata,
            )
            .with_trace_id(&opaque);
            let (first, second) = {
                let context = if current_parent {
                    Context::new().with_remote_span_context(parent(7))
                } else {
                    Context::new()
                };
                let context = context.with_value(PrivateValue);
                let context = if suppressed {
                    context.with_telemetry_suppressed()
                } else {
                    context
                };
                let _guard = context.attach();
                (
                    endpoint.handle(request, chigiri_http::BufferedBody::empty(), &NeverDeadline),
                    endpoint.handle(request, chigiri_http::BufferedBody::empty(), &NeverDeadline),
                )
            };
            let responses = std::thread::scope(|threads| {
                threads
                    .spawn(move || {
                        let worker = Context::new()
                            .with_remote_span_context(parent(9))
                            .with_value(PrivateValue);
                        let worker = if suppressed {
                            worker
                        } else {
                            worker.with_telemetry_suppressed()
                        };
                        let _guard = worker.attach();
                        let responses = [block_on(first), block_on(second)];
                        assert_eq!(*Context::current().span().span_context(), parent(9));
                        assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
                        responses
                    })
                    .join()
                    .unwrap()
            });
            let ended = records.take();
            let roots = ended
                .iter()
                .filter(|span| span.name == "graphql.request")
                .collect::<Vec<_>>();
            assert_eq!(roots.len(), if suppressed { 0 } else { 2 });
            assert_eq!(ended.len(), calls.load(Ordering::SeqCst));
            if suppressed {
                assert_eq!(calls.load(Ordering::SeqCst), 0);
            }
            let expected = parent(if current_parent { 7 } else { 8 });
            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 6);
            for (index, response) in responses.into_iter().enumerate() {
                assert_eq!(response.status, 200);
                let span = &seen[index * 3].0;
                assert_eq!(span.trace_id(), expected.trace_id());
                assert_eq!(
                    span.trace_state().header(),
                    if current_parent { "" } else { "vendor=value" }
                );
                assert!(
                    seen[index * 3..index * 3 + 3]
                        .iter()
                        .all(|(value, control)| value == span && *control == suppressed)
                );
                if !suppressed {
                    assert_eq!(roots[index].parent_span_id, expected.span_id());
                    assert_eq!(
                        super::scopes::context_factory(&ended, &roots[index].span_context),
                        *span
                    );
                }
                let resolver = if suppressed {
                    span.clone()
                } else {
                    super::scopes::execution(&ended, &roots[index].span_context)
                };
                let json: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
                assert_eq!(
                    json["data"]["greeting"],
                    format!(
                        "{}:{}:{}",
                        resolver.trace_id(),
                        resolver.span_id(),
                        suppressed
                    )
                );
            }
            assert_eq!(observed.errors.load(Ordering::SeqCst), 1);
        }
    }
}
