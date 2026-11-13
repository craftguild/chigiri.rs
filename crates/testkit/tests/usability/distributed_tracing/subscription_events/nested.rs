//! Event-local owning suppression must not alter the next event's setup baseline.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_http::{RequestPhase, SubscriptionTransport};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::FutureExt;

use super::super::{
    FixedSampling, recording,
    server::{Bridge, Server},
    workers,
};
use super::{behavior::Mode, client, fixture};

struct FirstEvent(Arc<AtomicUsize>);
impl chigiri::Plugin<()> for FirstEvent {
    fn operation<'a>(
        &'a self,
        context: chigiri::OperationContext<'a, ()>,
        next: chigiri::OperationNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        let current = Context::current();
        let current = if self.0.fetch_add(1, Ordering::SeqCst) == 1 {
            current.with_telemetry_suppressed()
        } else {
            current
        };
        Box::pin(next.run(context).with_context(current))
    }
}

struct Errors(Arc<AtomicUsize>);
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert!(Context::current().is_telemetry_suppressed());
        assert_eq!(event.code(), "INTERNAL_SERVER_ERROR");
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn nested_first_event_suppression_keeps_error_delivery_and_restores_later_events() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for profile in [
                SubscriptionTransport::WebSocket,
                SubscriptionTransport::ServerSentEvents,
                SubscriptionTransport::Multipart,
            ] {
                for baseline in [false, true] {
                    for has_parent in [false, true] {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(3),
                            run_case(bridge, profile, baseline, has_parent),
                        )
                        .await
                        .unwrap();
                    }
                }
            }
        }
    });
}

async fn run_case(
    bridge: Bridge,
    profile: SubscriptionTransport,
    baseline: bool,
    has_parent: bool,
) {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let drops = Arc::clone(&tracer.dropped);
    let operations = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let mut probe = fixture::Probe::new(profile);
    probe.mode = Mode::ErrorFirstResolver;
    let probe = Arc::new(probe);
    let endpoint = chigiri::Endpoint::builder(fixture::resolvers(Arc::clone(&probe)))
        .inject(|components| {
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .sampler(FixedSampling(chigiri::SamplingDecision::Record))
                .error_observer(Errors(Arc::clone(&errors)));
            components
                .plugins()
                .push_plugin(FirstEvent(Arc::clone(&operations)));
        })
        .context_factory(fixture::Factory(Arc::clone(&probe)))
        .build()
        .unwrap();
    let server = Server::start(endpoint.clone(), bridge, has_parent, baseline).await;
    let case = format!("nested/{bridge:?}/{profile:?}/suppressed={baseline}/parent={has_parent}");
    let data = client::events(server.address, profile, &case).await;
    server.stop().await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(chigiri::ShutdownOutcome::Drained)
    );
    assert_eq!(operations.load(Ordering::SeqCst), 3);
    assert_eq!(errors.load(Ordering::SeqCst), 1);
    assert_eq!(data[0]["errors"].as_array().unwrap().len(), 1);
    assert!(!data[0].to_string().contains("private event failure"));
    assert!(data[1].get("errors").is_none());
    assert_eq!(data[1]["data"]["ticks"]["value"], 2);
    let spans = records.take();
    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
    assert_eq!(drops.load(Ordering::SeqCst), calls.load(Ordering::SeqCst));
    assert!(spans.iter().all(|span| span.name != "chigiri.error"));
    let roots = spans
        .iter()
        .filter(|span| span.name == "graphql.subscription.event")
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), usize::from(!baseline) * 2);
    let executions = spans
        .iter()
        .filter(|span| span.name == "graphql.operation.execute")
        .collect::<Vec<_>>();
    assert_eq!(executions.len(), usize::from(!baseline));
    if !baseline {
        assert_eq!(
            executions[0].parent_span_id,
            roots[1].span_context.span_id()
        );
        assert_ne!(
            roots[0].span_context.trace_id(),
            roots[1].span_context.trace_id()
        );
    } else {
        assert!(spans.is_empty());
    }
    let seen = probe.seen.lock().unwrap();
    for sequence in [1, 2] {
        let phase = RequestPhase::SubscriptionEvent {
            transport: profile,
            sequence,
        };
        let resolver = seen
            .iter()
            .find(|seen| seen.phase == phase && seen.stage == "resolve")
            .unwrap();
        assert_eq!(resolver.suppressed, baseline || sequence == 1);
        assert!(!resolver.private);
        let factory = seen
            .iter()
            .find(|seen| seen.phase == phase && seen.stage == "factory-poll")
            .unwrap();
        assert_eq!(factory.suppressed, baseline);
        if !baseline {
            assert_eq!(
                resolver.span.trace_id(),
                roots[(sequence - 1) as usize].span_context.trace_id()
            );
        }
    }
    assert!(
        seen.iter()
            .filter(|seen| seen.stage.starts_with("source-"))
            .all(|seen| seen.suppressed == baseline)
    );
}
