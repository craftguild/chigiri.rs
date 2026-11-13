//! Deterministic sibling/request/endpoint interleaving moves live SDK work.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context as TaskContext, Poll, Waker};

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::TraceContextExt;

use super::super::support::{NeverDeadline, block_on};
use super::{PrivateValue, parent, query, recording, scopes};

fn assert_worker(byte: u8, suppressed: bool) {
    let current = Context::current();
    assert_eq!(*current.span().span_context(), parent(byte));
    assert_eq!(current.is_telemetry_suppressed(), suppressed);
    assert!(current.get::<PrivateValue>().is_some());
}

#[test]
fn concurrent_endpoints_requests_and_siblings_resume_on_another_native_thread() {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let endpoints = (0..2)
        .map(|_| {
            chigiri::Endpoint::builder(query::resolvers())
                .inject(|components| {
                    components
                        .observability()
                        .instrumentation(OpenTelemetryInstrumentation::new(tracer.clone()))
                        .field_instrumentation(true);
                })
                .without_context()
                .build()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut futures = Vec::new();
    for index in 0..3 {
        let context = Context::new()
            .with_remote_span_context(parent(8 - index as u8))
            .with_value(PrivateValue);
        let context = if index == 1 {
            context.with_telemetry_suppressed()
        } else {
            context
        };
        let _guard = context.attach();
        futures.push((
            index,
            endpoints[usize::from(index == 2)].execute(
                chigiri::GraphQlRequest::new("{ first: greeting second: greeting }"),
                (),
                &NeverDeadline,
            ),
        ));
    }
    let responses = std::thread::scope(|threads| {
        let futures = threads
            .spawn(move || {
                let worker = Context::new()
                    .with_remote_span_context(parent(9))
                    .with_value(PrivateValue)
                    .with_telemetry_suppressed();
                let _guard = worker.attach();
                let mut task = TaskContext::from_waker(Waker::noop());
                for (_, future) in &mut futures {
                    assert!(matches!(future.as_mut().poll(&mut task), Poll::Pending));
                    assert_worker(9, true);
                }
                futures
            })
            .join()
            .unwrap();
        threads
            .spawn(move || {
                let worker = Context::new()
                    .with_remote_span_context(parent(10))
                    .with_value(PrivateValue);
                let _guard = worker.attach();
                futures
                    .into_iter()
                    .rev()
                    .map(|(index, future)| {
                        let response = block_on(future);
                        assert_worker(10, false);
                        (index, response)
                    })
                    .collect::<Vec<_>>()
            })
            .join()
            .unwrap()
    });
    let spans = records.take();
    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
    let requests = spans
        .iter()
        .filter(|span| span.name == "graphql.request")
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    for (index, response) in responses {
        assert!(response.errors.is_empty());
        let data = chigiri_http::value_to_json(response.data.as_ref().unwrap()).unwrap();
        let selected = parent(8 - index as u8);
        if index == 1 {
            assert!(
                spans
                    .iter()
                    .all(|span| span.span_context.trace_id() != selected.trace_id())
            );
            let value = format!("{}:{}:true", selected.trace_id(), selected.span_id());
            assert_eq!(data["first"], value);
            assert_eq!(data["second"], value);
            continue;
        }
        let request = requests
            .iter()
            .find(|span| span.span_context.trace_id() == selected.trace_id())
            .unwrap();
        assert_eq!(request.parent_span_id, selected.span_id());
        let execution = scopes::execution(&spans, &request.span_context);
        let fields = spans
            .iter()
            .filter(|span| {
                span.name == "graphql.field.resolve" && span.parent_span_id == execution.span_id()
            })
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 2);
        assert_ne!(
            fields[0].span_context.span_id(),
            fields[1].span_context.span_id()
        );
        assert_ne!(data["first"], data["second"]);
        for field in fields {
            let value = serde_json::Value::String(format!(
                "{}:{}:false",
                field.span_context.trace_id(),
                field.span_context.span_id()
            ));
            assert!([&data["first"], &data["second"]].contains(&&value));
        }
    }
}
