//! Both adapters and all profiles give actual event work fresh linked root traces.

#[path = "subscription_events/behavior.rs"]
mod behavior;
#[path = "subscription_events/client.rs"]
mod client;
#[path = "subscription_events/fixture.rs"]
mod fixture;
#[path = "subscription_events/local.rs"]
mod local;
#[path = "subscription_events/native_cleanup.rs"]
mod native_cleanup;
#[path = "subscription_events/nested.rs"]
mod nested;
#[path = "subscription_events/verify.rs"]
mod verify;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_http::SubscriptionTransport;
use chigiri_opentelemetry::OpenTelemetryInstrumentation;

use super::{
    recording,
    server::{Bridge, Server},
    workers,
};

struct Sampling {
    calls: Arc<AtomicUsize>,
    setup_calls: usize,
    setup: chigiri::SamplingDecision,
    event: chigiri::SamplingDecision,
}
impl chigiri::Sampler for Sampling {
    fn sample(&self, _: chigiri::RequestIdentity<'_>) -> chigiri::SamplingDecision {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.setup_calls {
            self.setup
        } else {
            self.event
        }
    }
}

#[test]
fn native_events_have_fresh_roots_setup_links_and_event_wide_context() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for profile in [
                SubscriptionTransport::WebSocket,
                SubscriptionTransport::ServerSentEvents,
                SubscriptionTransport::Multipart,
            ] {
                for suppressed in [false, true] {
                    for has_parent in [false, true] {
                        for setup in [
                            chigiri::SamplingDecision::Record,
                            chigiri::SamplingDecision::Drop,
                        ] {
                            for event in [
                                chigiri::SamplingDecision::Record,
                                chigiri::SamplingDecision::Drop,
                            ] {
                                tokio::time::timeout(
                                    Duration::from_secs(3),
                                    run_case(bridge, profile, suppressed, has_parent, setup, event),
                                )
                                .await
                                .unwrap();
                            }
                        }
                    }
                }
            }
        }
    });
}

async fn run_case(
    bridge: Bridge,
    profile: SubscriptionTransport,
    suppressed: bool,
    has_parent: bool,
    setup: chigiri::SamplingDecision,
    event: chigiri::SamplingDecision,
) {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let dropped = Arc::clone(&tracer.dropped);
    let samples = Arc::new(AtomicUsize::new(0));
    let setup_calls = 1 + usize::from(profile == SubscriptionTransport::WebSocket);
    let probe = Arc::new(fixture::Probe::new(profile));
    let endpoint = chigiri::Endpoint::builder(fixture::resolvers(Arc::clone(&probe)))
        .inject(|components| {
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .sampler(Sampling {
                    calls: Arc::clone(&samples),
                    setup_calls,
                    setup,
                    event,
                });
        })
        .context_factory(fixture::Factory(Arc::clone(&probe)))
        .build()
        .unwrap();
    let server = Server::start(endpoint.clone(), bridge, has_parent, suppressed).await;
    let case = format!(
        "{bridge:?}/{profile:?}/suppressed={suppressed}/parent={has_parent}/setup={setup:?}/event={event:?}"
    );
    let data = client::events(server.address, profile, &case).await;
    server.stop().await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(chigiri::ShutdownOutcome::Drained)
    );
    assert_eq!(
        samples.load(Ordering::SeqCst),
        setup_calls + 2,
        "one decision per actual request/event"
    );
    let spans = records.take();
    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
    assert_eq!(dropped.load(Ordering::SeqCst), calls.load(Ordering::SeqCst));
    verify::check(&spans, &probe, &data, suppressed, has_parent, setup, event);
}
