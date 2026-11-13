//! Real idle subscriptions end setup before waiting without retaining app state.

#[path = "subscription_setup/client.rs"]
pub(super) mod client;
#[path = "subscription_setup/fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::trace::{SpanContext, SpanId};

use super::{
    FixedSampling, parent, recording, scopes,
    server::{Bridge, Server},
    workers,
};
use client::{Idle, Profile};

#[test]
fn native_subscription_setup_activates_factories_and_ends_before_idle_source_waiting() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for profile in [Profile::WebSocket, Profile::Sse, Profile::Multipart] {
                for suppressed in [false, true] {
                    for has_parent in [false, true] {
                        for decision in [
                            chigiri::SamplingDecision::Record,
                            chigiri::SamplingDecision::Drop,
                        ] {
                            tokio::time::timeout(
                                Duration::from_secs(3),
                                run_case(bridge, profile, suppressed, has_parent, decision),
                            )
                            .await
                            .unwrap();
                        }
                    }
                }
            }
        }
    });
}

async fn run_case(
    bridge: Bridge,
    profile: Profile,
    suppressed: bool,
    has_parent: bool,
    decision: chigiri::SamplingDecision,
) {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let dropped_spans = Arc::clone(&tracer.dropped);
    let probe = Arc::new(fixture::Probe::default());
    let endpoint = chigiri::Endpoint::builder(fixture::resolvers(Arc::clone(&probe)))
        .inject(|components| {
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .sampler(FixedSampling(decision));
        })
        .context_factory(fixture::Factory(Arc::clone(&probe)))
        .response_hook(fixture::Hook(Arc::clone(&probe)))
        .build()
        .unwrap();
    let server = Server::start_streaming(endpoint.clone(), bridge, has_parent, suppressed).await;
    let mut idle = Idle::open(server.address, profile).await;
    let opening = records.take();
    idle.subscribe(server.address, profile).await;
    while probe.polled.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    while dropped_spans.load(Ordering::SeqCst) != calls.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    let ended = records.take();
    assert_eq!(
        opening.len() + ended.len(),
        calls.load(Ordering::SeqCst),
        "all setup spans end before idle waiting"
    );
    let recorded = !suppressed && decision == chigiri::SamplingDecision::Record;
    let roots = ended
        .iter()
        .filter(|span| span.name == "graphql.request")
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), usize::from(recorded));
    let selected = if has_parent {
        parent(7)
    } else {
        SpanContext::NONE
    };
    let (setup, factory) = if recorded {
        assert_eq!(
            roots[0].parent_span_id,
            if has_parent {
                selected.span_id()
            } else {
                SpanId::INVALID
            }
        );
        (
            roots[0].span_context.clone(),
            scopes::context_factory(&ended, &roots[0].span_context),
        )
    } else {
        assert!(ended.is_empty());
        (selected.clone(), selected)
    };
    let hook = if recorded && !matches!(profile, Profile::WebSocket) {
        scopes::provider(&ended, &setup, "response-hook")
    } else {
        setup.clone()
    };
    {
        let seen = probe.seen.lock().unwrap();
        let factories = seen
            .iter()
            .filter(|seen| seen.stage.starts_with("factory-"))
            .collect::<Vec<_>>();
        assert_eq!(factories.len(), 4);
        for seen in factories {
            assert_eq!(
                seen.span, factory,
                "{bridge:?}/{profile:?}/{suppressed}/{has_parent}/{decision:?}/{seen:?}"
            );
        }
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.stage == "state-drop")
                .count(),
            1
        );
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.stage == "source-poll")
                .count(),
            1
        );
        for seen in seen.iter() {
            assert_eq!(seen.suppressed, suppressed, "{seen:?}");
            assert!(!seen.private, "{seen:?}");
            if seen.stage == "source-poll" || seen.stage.starts_with("completion") {
                assert_eq!(seen.span, setup, "{seen:?}");
            }
            if seen.stage == "state-drop" || seen.stage.starts_with("hook-") {
                assert_eq!(seen.span, hook, "{seen:?}");
            }
        }
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.stage == "completion")
                .count(),
            usize::from(!matches!(profile, Profile::WebSocket))
        );
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.stage == "completion-drop")
                .count(),
            1
        );
        assert_eq!(
            seen.iter()
                .filter(|seen| seen.stage.starts_with("hook-"))
                .count(),
            if matches!(profile, Profile::WebSocket) {
                0
            } else {
                4
            }
        );
    }
    idle.close().await;
    server.stop().await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(chigiri::ShutdownOutcome::Drained)
    );
    assert_eq!(probe.dropped.load(Ordering::SeqCst), 1);
    let seen = probe.seen.lock().unwrap();
    let dropped = seen
        .iter()
        .filter(|seen| seen.stage == "source-drop")
        .collect::<Vec<_>>();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].span, setup);
    assert_eq!(dropped[0].suppressed, suppressed);
    assert!(!dropped[0].private);
    assert!(records.take().is_empty());
    assert_eq!(
        dropped_spans.load(Ordering::SeqCst),
        calls.load(Ordering::SeqCst)
    );
    assert_eq!(opening.len() + ended.len(), calls.load(Ordering::SeqCst));
}
