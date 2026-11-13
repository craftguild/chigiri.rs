//! Distinct execution and opt-in sibling field spans surround actual work.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};
use opentelemetry_sdk::trace::SpanData;

use super::super::support::{NeverDeadline, block_on};
use super::{parent, query, recording};

pub(super) fn context_factory(spans: &[SpanData], request: &SpanContext) -> SpanContext {
    provider(spans, request, "context-construction")
}

pub(super) fn provider(spans: &[SpanData], request: &SpanContext, phase: &str) -> SpanContext {
    let matches = spans
        .iter()
        .filter(|span| {
            span.name == "chigiri.provider"
                && span.parent_span_id == request.span_id()
                && span.attributes.iter().any(|attribute| {
                    attribute.key.as_str() == "chigiri.phase" && attribute.value.as_str() == phase
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].span_context.trace_id(), request.trace_id());
    assert_ne!(matches[0].span_context.span_id(), request.span_id());
    matches[0].span_context.clone()
}

pub(super) fn execution(spans: &[SpanData], request: &SpanContext) -> SpanContext {
    let find = |name| {
        let children = spans
            .iter()
            .filter(|span| span.name == name && span.parent_span_id == request.span_id())
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 1, "one actual {name} child per request");
        assert_eq!(children[0].span_context.trace_id(), request.trace_id());
        assert!(children[0].end_time >= children[0].start_time);
        children[0].span_context.clone()
    };
    let resolution = find("graphql.operation.resolve");
    let execution = find("graphql.operation.execute");
    assert_ne!(resolution.span_id(), execution.span_id());
    assert_ne!(execution.span_id(), request.span_id());
    execution
}

#[test]
fn field_opt_in_creates_distinct_sibling_spans_around_pending_resolvers() {
    for enabled in [false, true] {
        for suppressed in [false, true] {
            let (_provider, tracer, records) = recording::provider();
            let calls = Arc::clone(&tracer.calls);
            let endpoint = chigiri::Endpoint::builder(query::resolvers())
                .inject(|components| {
                    components
                        .observability()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                        .field_instrumentation(enabled);
                })
                .without_context()
                .build()
                .unwrap();
            let future = {
                let context = Context::new().with_remote_span_context(parent(8));
                let context = if suppressed {
                    context.with_telemetry_suppressed()
                } else {
                    context
                };
                let _guard = context.attach();
                endpoint.execute(
                    chigiri::GraphQlRequest::new("{ first: greeting second: greeting }"),
                    (),
                    &NeverDeadline,
                )
            };
            let response = block_on(future);
            assert!(response.errors.is_empty());
            let data = chigiri_http::value_to_json(response.data.as_ref().unwrap()).unwrap();
            let spans = records.take();
            assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
            let fields = spans
                .iter()
                .filter(|span| span.name == "graphql.field.resolve")
                .collect::<Vec<_>>();
            assert_eq!(fields.len(), if enabled && !suppressed { 2 } else { 0 });
            let expected = if suppressed {
                assert!(spans.is_empty());
                parent(8)
            } else {
                let requests = spans
                    .iter()
                    .filter(|span| span.name == "graphql.request")
                    .collect::<Vec<_>>();
                assert_eq!(requests.len(), 1);
                let execution = execution(&spans, &requests[0].span_context);
                for field in &fields {
                    assert_eq!(field.parent_span_id, execution.span_id());
                    assert_eq!(field.span_context.trace_id(), execution.trace_id());
                }
                if enabled {
                    assert_ne!(
                        fields[0].span_context.span_id(),
                        fields[1].span_context.span_id()
                    );
                }
                execution
            };
            if enabled && !suppressed {
                let actual = [
                    data["first"].as_str().unwrap(),
                    data["second"].as_str().unwrap(),
                ];
                for field in fields {
                    let value = format!(
                        "{}:{}:false",
                        field.span_context.trace_id(),
                        field.span_context.span_id()
                    );
                    assert_eq!(actual.iter().filter(|actual| **actual == value).count(), 1);
                }
            } else {
                let value = format!(
                    "{}:{}:{}",
                    expected.trace_id(),
                    expected.span_id(),
                    suppressed
                );
                assert_eq!(data["first"], value);
                assert_eq!(data["second"], value);
            }
        }
    }
}
