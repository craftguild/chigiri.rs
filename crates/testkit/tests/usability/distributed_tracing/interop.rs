//! Standard contextual tracing and a real application-owned downstream carrier.

#[path = "interop/downstream.rs"]
mod downstream;
#[path = "interop/query.rs"]
mod query;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{FutureExt, SpanContext, TraceContextExt, TraceFlags};
use tracing::Instrument;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::layer::SubscriberExt;

use super::super::support::NeverDeadline;
use super::{FixedSampling, PrivateValue, client, parent, recording};

#[test]
fn ordinary_contextual_tracing_joins_sdk_even_under_middleware_and_reaches_downstream() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for middleware in [false, true] {
            for sampled in [false, true] {
                for decision in [
                    chigiri::SamplingDecision::Record,
                    chigiri::SamplingDecision::Drop,
                ] {
                    let (_provider, tracer, records) = recording::provider();
                    let calls = Arc::clone(&tracer.calls);
                    let (address, downstream) = downstream::start(tracer.clone()).await;
                    let seen = Arc::new(Mutex::new(None));
                    let endpoint =
                        chigiri::Endpoint::builder(query::resolvers(address, Arc::clone(&seen)))
                            .inject(|components| {
                                components
                                    .observability()
                                    .instrumentation(OpenTelemetryInstrumentation::new(
                                        tracer.clone(),
                                    ))
                                    .sampler(FixedSampling(decision));
                            })
                            .without_context()
                            .build()
                            .unwrap();
                    let subscriber = tracing_subscriber::Registry::default().with(
                        tracing_opentelemetry::layer()
                            .with_tracer(tracer)
                            .with_context_activation(true),
                    );
                    let selected = parent(8);
                    let selected = SpanContext::new(
                        selected.trace_id(),
                        selected.span_id(),
                        if sampled {
                            TraceFlags::SAMPLED
                        } else {
                            TraceFlags::default()
                        },
                        true,
                        selected.trace_state().clone(),
                    );
                    let incoming = Context::new()
                        .with_remote_span_context(selected.clone())
                        .with_value(PrivateValue);
                    let response = async {
                        let span = if middleware {
                            tracing::info_span!("application.middleware")
                        } else {
                            tracing::Span::none()
                        };
                        async {
                            endpoint
                                .execute(
                                    chigiri::GraphQlRequest::new("{ greeting }"),
                                    (),
                                    &NeverDeadline,
                                )
                                .await
                        }
                        .instrument(span)
                        .await
                    }
                    .with_context(incoming)
                    .with_subscriber(subscriber)
                    .await;
                    assert!(response.errors.is_empty());
                    let downstream = tokio::time::timeout(Duration::from_secs(2), downstream)
                        .await
                        .unwrap()
                        .unwrap();
                    let seen = seen.lock().unwrap().take().expect("ordinary resolver ran");
                    assert_eq!(seen.application.trace_id(), downstream.parent.trace_id());
                    assert_eq!(seen.application.span_id(), downstream.parent.span_id());
                    assert_eq!(
                        seen.application.trace_flags(),
                        downstream.parent.trace_flags()
                    );
                    assert_eq!(
                        seen.application.trace_state(),
                        downstream.parent.trace_state()
                    );
                    assert!(downstream.parent.is_remote());
                    assert_eq!(seen.application.trace_id(), selected.trace_id());
                    assert_eq!(downstream.child.trace_id(), selected.trace_id());
                    assert_ne!(downstream.child.span_id(), seen.application.span_id());
                    for context in [
                        &seen.execution,
                        &seen.application,
                        &downstream.parent,
                        &downstream.child,
                    ] {
                        assert!(context.is_valid());
                        assert_eq!(context.is_sampled(), sampled);
                    }
                    assert!(!Context::current().span().span_context().is_valid());
                    assert!(!Context::current().is_telemetry_suppressed());
                    let spans = records.take();
                    if !sampled {
                        assert!(
                            spans.is_empty(),
                            "parent-based non-recording remains non-recording"
                        );
                        continue;
                    }
                    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
                    let one = |name| {
                        let found = spans
                            .iter()
                            .filter(|span| span.name == name)
                            .collect::<Vec<_>>();
                        assert_eq!(found.len(), 1, "{name}");
                        found[0]
                    };
                    let upstream = if middleware {
                        let span = one("application.middleware");
                        assert_eq!(span.parent_span_id, selected.span_id());
                        span.span_context.clone()
                    } else {
                        selected
                    };
                    if decision == chigiri::SamplingDecision::Record {
                        let request = one("graphql.request");
                        assert_eq!(request.parent_span_id, upstream.span_id());
                        assert_eq!(
                            seen.execution,
                            super::scopes::execution(&spans, &request.span_context)
                        );
                    } else {
                        assert!(spans.iter().all(|span| !span.name.starts_with("graphql.")));
                        assert_eq!(seen.execution, upstream);
                    }
                    let application = one("application.resolver");
                    assert_eq!(application.span_context, seen.application);
                    assert_eq!(application.parent_span_id, seen.execution.span_id());
                    assert_eq!(
                        one("application.downstream").parent_span_id,
                        seen.application.span_id()
                    );
                }
            }
        }
    });
}
