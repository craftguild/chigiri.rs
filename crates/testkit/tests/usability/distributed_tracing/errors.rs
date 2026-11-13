//! Nested owning suppression governs SDK children and error conversion.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{FutureExt, TraceContextExt};

use super::super::support::{NeverDeadline, block_on};
use super::{FixedSampling, parent, query, recording};

struct NestedSuppression {
    request: bool,
    operation: bool,
}
impl chigiri::Plugin<()> for NestedSuppression {
    fn request<'a>(
        &'a self,
        context: chigiri::RequestContext<'a, ()>,
        next: chigiri::RequestNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        let current = Context::current();
        let current = if self.request {
            current.with_telemetry_suppressed()
        } else {
            current
        };
        Box::pin(next.run(context).with_context(current))
    }

    fn operation<'a>(
        &'a self,
        context: chigiri::OperationContext<'a, ()>,
        next: chigiri::OperationNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        let current = Context::current();
        let current = if self.operation {
            current.with_telemetry_suppressed()
        } else {
            current
        };
        Box::pin(next.run(context).with_context(current))
    }
}

struct Errors {
    suppressed: bool,
    calls: Arc<AtomicUsize>,
}
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert_eq!(
            Context::current().is_telemetry_suppressed(),
            self.suppressed
        );
        assert_eq!(event.code(), "INTERNAL_SERVER_ERROR");
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn nested_request_and_operation_suppression_keeps_independent_error_delivery() {
    for (outer, request, operation) in [
        (false, false, false),
        (false, true, false),
        (false, false, true),
        (true, false, false),
    ] {
        let nested = request || operation;
        for sampling in [
            chigiri::SamplingDecision::Record,
            chigiri::SamplingDecision::Drop,
        ] {
            let (_provider, tracer, records) = recording::provider();
            let calls = Arc::clone(&tracer.calls);
            let errors = Arc::new(AtomicUsize::new(0));
            let endpoint = chigiri::Endpoint::builder(query::failing_resolvers())
                .inject(|components| {
                    components
                        .observability()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                        .field_instrumentation(true)
                        .sampler(FixedSampling(sampling))
                        .error_observer(Errors {
                            suppressed: outer || nested,
                            calls: Arc::clone(&errors),
                        });
                    components
                        .plugins()
                        .push_plugin(NestedSuppression { request, operation });
                })
                .without_context()
                .build()
                .unwrap();
            let future = {
                let context = Context::new().with_remote_span_context(parent(8));
                let context = if outer {
                    context.with_telemetry_suppressed()
                } else {
                    context
                };
                let _guard = context.attach();
                endpoint.execute(
                    chigiri::GraphQlRequest::new("{ greeting }"),
                    (),
                    &NeverDeadline,
                )
            };
            let worker = Context::new().with_remote_span_context(parent(9));
            let _guard = worker.attach();
            let response = block_on(future);
            assert_eq!(errors.load(Ordering::SeqCst), 1);
            assert_eq!(response.errors.len(), 1);
            assert!(!format!("{response:?}").contains("private error source"));
            assert_eq!(*Context::current().span().span_context(), parent(9));
            assert!(!Context::current().is_telemetry_suppressed());
            let spans = records.take();
            assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
            if outer {
                assert!(spans.is_empty());
            } else if operation {
                assert!(spans.iter().all(|span| !matches!(
                    span.name.as_ref(),
                    "graphql.operation.execute" | "graphql.field.resolve" | "chigiri.error"
                )));
                assert_eq!(
                    spans
                        .iter()
                        .filter(|span| span.name == "graphql.request")
                        .count(),
                    usize::from(sampling == chigiri::SamplingDecision::Record)
                );
                if sampling == chigiri::SamplingDecision::Drop {
                    assert!(spans.is_empty());
                }
            } else if nested {
                assert!(spans.iter().all(|span| span.name == "graphql.request"));
                assert_eq!(
                    spans.len(),
                    usize::from(sampling == chigiri::SamplingDecision::Record)
                );
            } else {
                let diagnostic = spans
                    .iter()
                    .filter(|span| span.name == "chigiri.error")
                    .collect::<Vec<_>>();
                assert_eq!(diagnostic.len(), 1);
                assert_eq!(diagnostic[0].span_context.trace_id(), parent(8).trace_id());
                if sampling == chigiri::SamplingDecision::Drop {
                    assert_eq!(spans.len(), 1);
                    assert_eq!(diagnostic[0].parent_span_id, parent(8).span_id());
                }
            }
        }
    }
}
