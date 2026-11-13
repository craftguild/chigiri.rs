//! Active native events retain owned context during disconnect and opaque-payload disposal.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_http::{RequestPhase, SubscriptionTransport};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;

use super::super::subscription_setup::client::{Idle, Profile};
use super::super::{
    recording,
    server::{Bridge, Server},
    workers,
};
use super::{behavior::Mode, fixture};

struct Errors {
    calls: Arc<AtomicUsize>,
    suppressed: bool,
}
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert_eq!(
            Context::current().is_telemetry_suppressed(),
            self.suppressed
        );
        assert!(
            Context::current()
                .get::<super::super::PrivateValue>()
                .is_none()
        );
        assert_eq!(event.code(), "INTERNAL_SERVER_ERROR");
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pending_native_event_disconnect_releases_every_scope_and_physical_owner() {
    run(false);
}

#[test]
fn native_event_panic_payload_is_disposed_under_its_event_control_and_trace() {
    run(true);
}

fn run(panic: bool) {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for profile in [Profile::WebSocket, Profile::Sse, Profile::Multipart] {
                for suppressed in [false, true] {
                    for has_parent in [false, true] {
                        tokio::time::timeout(
                            Duration::from_secs(3),
                            run_case(bridge, profile, suppressed, has_parent, panic),
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
    profile: Profile,
    suppressed: bool,
    has_parent: bool,
    panic: bool,
) {
    let selected = match profile {
        Profile::WebSocket => SubscriptionTransport::WebSocket,
        Profile::Sse => SubscriptionTransport::ServerSentEvents,
        Profile::Multipart => SubscriptionTransport::Multipart,
    };
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let drops = Arc::clone(&tracer.dropped);
    let errors = Arc::new(AtomicUsize::new(0));
    let mut probe = fixture::Probe::new(selected);
    probe.mode = if panic {
        Mode::OwnedPanicResolver
    } else {
        Mode::PendingResolver
    };
    if panic {
        probe.panic_release = Some(tokio::sync::Notify::new());
    }
    let probe = Arc::new(probe);
    let endpoint = chigiri::Endpoint::builder(fixture::resolvers(Arc::clone(&probe)))
        .inject(|components| {
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .error_observer(Errors {
                    calls: Arc::clone(&errors),
                    suppressed,
                });
        })
        .context_factory(fixture::Factory(Arc::clone(&probe)))
        .build()
        .unwrap();
    let server = if panic {
        Server::start_failed_stream(endpoint.clone(), bridge, has_parent, suppressed).await
    } else {
        Server::start_streaming(endpoint.clone(), bridge, has_parent, suppressed).await
    };
    let mut client = Idle::open(server.address, profile).await;
    client.subscribe(server.address, profile).await;
    loop {
        if probe
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|seen| seen.stage == "resolve")
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    if panic {
        // This case exercises failure after the native transport accepted its head.
        probe.panic_release.as_ref().unwrap().notify_one();
        while errors.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        client.failed().await;
    } else {
        client.close().await;
    }
    while !probe
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|seen| seen.stage == "source-drop")
    {
        tokio::task::yield_now().await;
    }
    server.stop().await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(if panic {
            chigiri::ShutdownOutcome::InfrastructureFailure
        } else {
            chigiri::ShutdownOutcome::Drained
        })
    );
    assert_eq!(errors.load(Ordering::SeqCst), usize::from(panic));
    let spans = records.take();
    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
    assert_eq!(drops.load(Ordering::SeqCst), calls.load(Ordering::SeqCst));
    let events = spans
        .iter()
        .filter(|span| span.name == "graphql.subscription.event")
        .collect::<Vec<_>>();
    assert_eq!(events.len(), usize::from(!suppressed));
    let seen = probe.seen.lock().unwrap();
    for stage in ["source-drop", "state-drop", "resolver-drop", "event-drop"] {
        let matching = seen
            .iter()
            .filter(|seen| {
                seen.stage == stage
                    && (stage == "source-drop"
                        || matches!(seen.phase, RequestPhase::SubscriptionEvent { .. }))
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{bridge:?}/{profile:?}/{stage}");
        assert_eq!(matching[0].suppressed, suppressed, "{stage}");
        assert!(!matching[0].private, "{stage}");
        if stage != "source-drop" && !suppressed {
            assert_eq!(
                matching[0].span.trace_id(),
                events[0].span_context.trace_id(),
                "{stage}"
            );
        }
    }
    let payloads = seen
        .iter()
        .filter(|seen| seen.stage == "panic-payload-drop")
        .collect::<Vec<_>>();
    assert_eq!(payloads.len(), usize::from(panic));
    if panic {
        assert_eq!(payloads[0].suppressed, suppressed);
        assert!(!payloads[0].private);
        if !suppressed {
            assert_eq!(
                payloads[0].span.trace_id(),
                events[0].span_context.trace_id()
            );
        }
    }
    if suppressed {
        assert!(spans.is_empty());
    }
}
