//! Public generated consumers exercise actual direct execution across handoff.

#[path = "../managed_shutdown/client.rs"]
mod client;

#[path = "distributed_tracing/concurrency.rs"]
mod concurrency;
#[path = "distributed_tracing/disposal.rs"]
mod disposal;
#[path = "distributed_tracing/errors.rs"]
mod errors;
#[path = "distributed_tracing/http.rs"]
mod http;
#[path = "distributed_tracing/http_disposal.rs"]
mod http_disposal;
#[path = "distributed_tracing/interop.rs"]
mod interop;
#[path = "distributed_tracing/native.rs"]
mod native;
#[path = "distributed_tracing/query.rs"]
mod query;
#[path = "distributed_tracing/recording.rs"]
mod recording;
#[path = "distributed_tracing/scopes.rs"]
mod scopes;
#[path = "distributed_tracing/native/server.rs"]
mod server;
#[path = "distributed_tracing/subscription_events.rs"]
mod subscription_events;
#[path = "distributed_tracing/subscription_setup.rs"]
mod subscription_setup;
#[path = "distributed_tracing/subscription_source.rs"]
mod subscription_source;
#[path = "distributed_tracing/websocket.rs"]
mod websocket;
#[path = "distributed_tracing/native/workers.rs"]
mod workers;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};

use super::support::{NeverDeadline, block_on};

#[derive(Clone)]
struct PrivateValue;

fn parent(byte: u8) -> SpanContext {
    SpanContext::new(
        TraceId::from_bytes([byte; 16]),
        SpanId::from_bytes([byte; 8]),
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    )
}

#[derive(Clone, Default)]
struct Observed {
    lifecycle: Arc<AtomicUsize>,
    errors: Arc<AtomicUsize>,
}

impl chigiri::LifecycleObserver for Observed {
    fn observe(&self, event: chigiri::LifecycleEvent<'_>) {
        assert_correlation(event.identity());
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
    }
}
impl chigiri::ErrorObserver for Observed {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert_correlation(event.identity());
        self.errors.fetch_add(1, Ordering::SeqCst);
    }
}

fn assert_correlation(identity: chigiri::RequestIdentity<'_>) {
    let current = Context::current();
    let span = current.span();
    let actual = span.span_context();
    assert_eq!(
        identity.trace_id().map(|value| value.as_str().to_owned()),
        actual.is_valid().then(|| actual.trace_id().to_string())
    );
}

struct FirstIdentityOnly(AtomicUsize);
impl chigiri::RequestIdFactory for FirstIdentityOnly {
    fn create(&self) -> Result<chigiri::RequestId, chigiri::RequestIdFactoryError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            chigiri::RequestId::new("direct-consumer").map_err(chigiri::RequestIdFactoryError::new)
        } else {
            Err(chigiri::RequestIdFactoryError::new(std::io::Error::other(
                "identity deliberately unavailable",
            )))
        }
    }
}

struct FixedSampling(chigiri::SamplingDecision);
impl chigiri::Sampler for FixedSampling {
    fn sample(&self, _: chigiri::RequestIdentity<'_>) -> chigiri::SamplingDecision {
        self.0
    }
}

#[test]
fn one_registration_propagates_real_direct_context_through_thread_polling_and_id_failure() {
    let mut baseline_observers = None;
    for suppressed in [false, true] {
        for has_parent in [false, true] {
            for decision in [
                chigiri::SamplingDecision::Record,
                chigiri::SamplingDecision::Drop,
            ] {
                let (_provider, tracer, records) = recording::provider();
                let calls = Arc::clone(&tracer.calls);
                let observed = Observed::default();
                let endpoint = chigiri::Endpoint::builder(query::resolvers())
                    .inject(|components| {
                        components
                            .observability()
                            .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                            .request_id_factory(FirstIdentityOnly(AtomicUsize::new(0)))
                            .sampler(FixedSampling(decision))
                            .lifecycle_observer(observed.clone())
                            .error_observer(observed.clone());
                    })
                    .without_context()
                    .build()
                    .unwrap();
                let (first, second) = {
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
                    (
                        endpoint.execute(
                            chigiri::GraphQlRequest::new("{ greeting }"),
                            (),
                            &NeverDeadline,
                        ),
                        endpoint.execute(
                            chigiri::GraphQlRequest::new("{ greeting }"),
                            (),
                            &NeverDeadline,
                        ),
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
                            let current = Context::current();
                            assert_eq!(*current.span().span_context(), parent(9));
                            assert_eq!(current.is_telemetry_suppressed(), !suppressed);
                            assert!(current.get::<PrivateValue>().is_some());
                            responses
                        })
                        .join()
                        .unwrap()
                });
                let ended = records.take();
                let recording = !suppressed && decision == chigiri::SamplingDecision::Record;
                assert_eq!(
                    ended.len(),
                    if !suppressed {
                        calls.load(Ordering::SeqCst)
                    } else {
                        0
                    }
                );
                if suppressed {
                    assert_eq!(calls.load(Ordering::SeqCst), 0);
                }
                if decision == chigiri::SamplingDecision::Drop {
                    assert!(ended.iter().all(|span| span.name == "chigiri.error"));
                }
                let roots = ended
                    .iter()
                    .filter(|span| span.name == "graphql.request")
                    .collect::<Vec<_>>();
                assert_eq!(roots.len(), if recording { 2 } else { 0 });
                if recording {
                    assert_ne!(
                        roots[0].span_context.span_id(),
                        roots[1].span_context.span_id()
                    );
                    if !has_parent {
                        assert_ne!(
                            roots[0].span_context.trace_id(),
                            roots[1].span_context.trace_id()
                        );
                    }
                }
                for (index, response) in responses.into_iter().enumerate() {
                    assert!(response.errors.is_empty(), "{:?}", response.errors);
                    let data =
                        chigiri_http::value_to_json(response.data.as_ref().unwrap()).unwrap();
                    let (trace, span) = if recording {
                        let root = roots[index];
                        assert_eq!(
                            root.parent_span_id,
                            if has_parent {
                                parent(8).span_id()
                            } else {
                                SpanId::INVALID
                            }
                        );
                        if has_parent {
                            assert_eq!(root.span_context.trace_id(), parent(8).trace_id());
                        }
                        let execution = scopes::execution(&ended, &root.span_context);
                        (execution.trace_id(), execution.span_id())
                    } else if has_parent {
                        (parent(8).trace_id(), parent(8).span_id())
                    } else {
                        (TraceId::INVALID, SpanId::INVALID)
                    };
                    assert_eq!(data["greeting"], format!("{trace}:{span}:{suppressed}"));
                }
                let counts = (
                    observed.lifecycle.load(Ordering::SeqCst),
                    observed.errors.load(Ordering::SeqCst),
                );
                assert_eq!(counts.1, 1);
                assert_eq!(*baseline_observers.get_or_insert(counts), counts);
            }
        }
    }
}
