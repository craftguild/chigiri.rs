//! SDK-owned factory cancellation/unwind restores both worker ambient values.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Waker};

use chigiri::spi::{ApplicationMetadata, RequestHeader};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt};

use super::super::support::NeverDeadline;
use super::{PrivateValue, parent, query, recording};

struct Probe {
    selected: SpanContext,
    suppressed: bool,
    drops: Arc<AtomicUsize>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        let current = Context::current();
        assert_eq!(*current.span().span_context(), self.selected);
        assert_eq!(current.is_telemetry_suppressed(), self.suppressed);
        assert!(current.get::<PrivateValue>().is_none());
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Factory {
    panic: bool,
    seen: Arc<Mutex<Option<SpanContext>>>,
    drops: Arc<AtomicUsize>,
}

impl chigiri_http::ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        _: chigiri_http::RequestContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::PreparedContext<(), ()>, chigiri::PrepareError>>
    {
        let current = Context::current();
        assert!(current.get::<PrivateValue>().is_none());
        let selected = current.span().span_context().clone();
        assert!(
            self.seen
                .lock()
                .unwrap()
                .replace(selected.clone())
                .is_none()
        );
        let probe = Probe {
            selected,
            suppressed: current.is_telemetry_suppressed(),
            drops: Arc::clone(&self.drops),
        };
        Box::pin(async move {
            let _probe = probe;
            assert!(!self.panic, "deliberate factory poll unwind");
            std::future::pending().await
        })
    }
}

#[test]
fn factory_future_drop_and_unwind_use_owned_provider_context_with_and_without_parent() {
    for has_parent in [false, true] {
        for suppressed in [false, true] {
            for panic in [false, true] {
                let (_provider, tracer, records) = recording::provider();
                let calls = Arc::clone(&tracer.calls);
                let seen = Arc::new(Mutex::new(None));
                let drops = Arc::new(AtomicUsize::new(0));
                let endpoint = chigiri::Endpoint::builder(query::resolvers())
                    .inject(|components| {
                        components
                            .observability()
                            .instrumentation(OpenTelemetryInstrumentation::new(tracer));
                    })
                    .context_factory(Factory {
                        panic,
                        seen: Arc::clone(&seen),
                        drops: Arc::clone(&drops),
                    })
                    .build()
                    .unwrap();
                let metadata = ApplicationMetadata::default();
                let headers = [
                    RequestHeader::new("accept", b"application/graphql-response+json"),
                    RequestHeader::new("x-chigiri-preflight", b"1"),
                ];
                let request = chigiri::TransportRequest::new(
                    "GET",
                    "/graphql",
                    Some("query=%7B%20greeting%20%7D"),
                    &headers,
                    None,
                    &metadata,
                );
                let mut future = {
                    let context = if has_parent {
                        Context::new().with_remote_span_context(parent(8))
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
                    endpoint.handle(request, chigiri_http::BufferedBody::empty(), &NeverDeadline)
                };
                let worker = Context::new()
                    .with_remote_span_context(parent(9))
                    .with_value(PrivateValue);
                let worker = if suppressed {
                    worker
                } else {
                    worker.with_telemetry_suppressed()
                };
                let _guard = worker.attach();
                let mut task = TaskContext::from_waker(Waker::noop());
                let polled = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut task)));
                if panic {
                    assert!(polled.is_err());
                } else {
                    assert!(polled.unwrap().is_pending());
                }
                drop(future);
                let current = Context::current();
                assert_eq!(*current.span().span_context(), parent(9));
                assert_eq!(current.is_telemetry_suppressed(), !suppressed);
                assert!(current.get::<PrivateValue>().is_some());
                assert_eq!(drops.load(Ordering::SeqCst), 1);
                let selected = seen.lock().unwrap().clone().expect("factory started");
                let spans = records.take();
                assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
                if suppressed {
                    assert!(spans.is_empty());
                    assert_eq!(
                        selected,
                        if has_parent {
                            parent(8)
                        } else {
                            SpanContext::empty_context()
                        }
                    );
                } else {
                    let roots = spans
                        .iter()
                        .filter(|span| span.name == "graphql.request")
                        .collect::<Vec<_>>();
                    assert_eq!(roots.len(), 1);
                    assert_eq!(
                        roots[0].parent_span_id,
                        if has_parent {
                            parent(8).span_id()
                        } else {
                            SpanId::INVALID
                        }
                    );
                    assert_eq!(
                        selected,
                        super::scopes::context_factory(&spans, &roots[0].span_context)
                    );
                    for span in spans.iter().filter(|span| {
                        span.span_context == selected || span.name == "graphql.request"
                    }) {
                        assert!(
                            span.attributes
                                .iter()
                                .any(|attribute| attribute.key.as_str() == "chigiri.result"
                                    && attribute.value.as_str() == "cancelled")
                        );
                    }
                }
            }
        }
    }
}
