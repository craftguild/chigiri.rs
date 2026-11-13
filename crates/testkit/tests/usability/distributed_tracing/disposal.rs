//! Unstarted direct work still destroys SDK-owned providers under captured control.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::{PrivateValue, parent, query, recording};

struct Expected {
    parent: SpanContext,
    suppressed: bool,
    stages: Arc<Mutex<Vec<&'static str>>>,
}

impl Expected {
    fn check(&self, stage: &'static str) {
        let current = Context::current();
        assert_eq!(*current.span().span_context(), self.parent);
        assert_eq!(current.is_telemetry_suppressed(), self.suppressed);
        assert!(current.get::<PrivateValue>().is_none());
        self.stages.lock().unwrap().push(stage);
    }
}

struct DeadlineProbe(Arc<Expected>);

impl Future for DeadlineProbe {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<()> {
        self.0.check("poll");
        Poll::Pending
    }
}

impl Drop for DeadlineProbe {
    fn drop(&mut self) {
        self.0.check("drop");
    }
}

struct Deadlines(Arc<Expected>);

impl chigiri::DeadlineFactory for Deadlines {
    fn sleep(&self, _: Duration) -> chigiri::BoxFuture<'_, ()> {
        self.0.check("construct");
        Box::pin(DeadlineProbe(Arc::clone(&self.0)))
    }
}

#[test]
fn dropping_an_unpolled_request_preserves_parentless_and_parented_control_without_starting_spans() {
    for suppressed in [false, true] {
        for has_parent in [false, true] {
            let (_provider, tracer, records) = recording::provider();
            let calls = Arc::clone(&tracer.calls);
            let endpoint = chigiri::Endpoint::builder(query::resolvers())
                .inject(|components| {
                    components
                        .observability()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer));
                })
                .without_context()
                .build()
                .unwrap();
            let stages = Arc::new(Mutex::new(Vec::new()));
            let expected = if has_parent {
                parent(8)
            } else {
                SpanContext::NONE
            };
            let deadlines = Deadlines(Arc::new(Expected {
                parent: expected.clone(),
                suppressed,
                stages: Arc::clone(&stages),
            }));
            let future = {
                let context = Context::new()
                    .with_remote_span_context(expected)
                    .with_value(PrivateValue);
                let context = if suppressed {
                    context.with_telemetry_suppressed()
                } else {
                    context
                };
                let _guard = context.attach();
                endpoint.execute(chigiri::GraphQlRequest::new("{ greeting }"), (), &deadlines)
            };
            std::thread::scope(|threads| {
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
                        drop(future);
                        let current = Context::current();
                        assert_eq!(*current.span().span_context(), parent(9));
                        assert_eq!(current.is_telemetry_suppressed(), !suppressed);
                        assert!(current.get::<PrivateValue>().is_some());
                    })
                    .join()
                    .unwrap()
            });
            assert_eq!(*stages.lock().unwrap(), ["construct", "drop"]);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert!(records.take().is_empty());
        }
    }
}
