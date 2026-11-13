//! Automatic HTTP ownership and eligible event drain, using concrete workers.

mod completion;
mod context_ready_drop;
mod failures;
mod races;
mod validation;
mod websocket;

use std::future::poll_fn;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader,
    SubscriptionTransport, TransportRequest, WebSocketHttpVersion,
};

use super::super::harness::{event_gate, session_endpoint};
use super::super::state::{AppContext, SourceStep, State, count};
use super::support::{VALID, profiles, run, runtime};
use crate::managed::{ManagedHttpResponse, ManagedService};
use crate::transport::streaming::supervisor::HttpStreamingBody;
use crate::{Endpoint, ShutdownOptions, ShutdownOutcome, SubscriptionLimits};

struct Timers;
static TIMERS: Timers = Timers;
impl DeadlineFactory for Timers {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

async fn body(
    endpoint: &Endpoint<AppContext>,
    service: &ManagedService,
    profile: SubscriptionTransport,
) -> HttpStreamingBody {
    let accept = match profile {
        SubscriptionTransport::ServerSentEvents => "text/event-stream",
        SubscriptionTransport::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        _ => unreachable!(),
    };
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept.as_bytes()),
        RequestHeader::new("authorization", b"original-secret"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    match endpoint
        .__handle_managed_http(
            service.request().unwrap(),
            request,
            BufferedBody::new(VALID.to_vec()),
            WebSocketHttpVersion::Http11,
            &TIMERS,
        )
        .await
    {
        ManagedHttpResponse::Streaming(response) => {
            assert_eq!(response.status(), 200);
            response.into_body()
        }
        _ => panic!("managed source must commit a supervised body"),
    }
}

async fn until(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded managed worker progress");
}

async fn until_pending_source(probe: &State) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !probe.source_pending_observed.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded managed source Pending observation");
}

async fn next(body: &mut HttpStreamingBody) -> Option<String> {
    poll_fn(|cx| body.poll_next(cx)).await.map(|frame| {
        let frame = frame.unwrap().accept().unwrap();
        String::from_utf8(frame.as_ref().to_vec()).unwrap()
    })
}

#[test]
fn managed_graceful_cancels_pending_source_without_waiting_for_natural_completion() {
    let runtime = runtime();
    for profile in profiles().into_iter().cycle().take(16) {
        let probe = State::new(vec![SourceStep::Pending]);
        let endpoint = session_endpoint(probe.clone(), SubscriptionLimits::default());
        let stage = std::cell::Cell::new("service activation");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(&runtime, async {
                let service = endpoint.__managed_service().unwrap();
                stage.set("HTTP preparation");
                let mut body = body(&endpoint, &service, profile).await;
                stage.set("initial body frame");
                drop(next(&mut body).await.unwrap());
                stage.set("pending source demand");
                until_pending_source(&probe).await;
                stage.set("SDK graceful drain");
                tokio::time::timeout(Duration::from_secs(2), endpoint.shutdown_on(async {}, ShutdownOptions::graceful())).await.unwrap_or_else(|_| {
                    panic!("live SDK drain timeout: profile={profile:?}, phase={:?}, outcome={:?}, requests_idle={}, source_polls={}, source_drops={}", endpoint.managed_control().phase(), endpoint.shutdown_outcome(), endpoint.managed_control().requests_idle(), count(&probe.counts.source_polls), count(&probe.counts.source_drops))
                });
                assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
                assert!(service.request().is_err());
                stage.set("final body EOF");
                assert!(next(&mut body).await.is_none());
                assert_eq!(count(&probe.counts.source_drops), 1);
                assert_eq!(count(&probe.counts.factory_calls), 1);
                assert_eq!(count(&probe.counts.hook_calls), 1);
                assert_eq!(count(&probe.counts.completion_calls), 1);
            })
        }));
        if let Err(payload) = result {
            eprintln!(
                "managed pending-source timeout: profile={profile:?}, stage={}, outcome={:?}, requests_idle={}, source_polls={}, source_drops={}",
                stage.get(),
                endpoint.shutdown_outcome(),
                endpoint.managed_control().requests_idle(),
                count(&probe.counts.source_polls),
                count(&probe.counts.source_drops)
            );
            std::panic::resume_unwind(payload);
        }
    }
}

#[test]
fn managed_graceful_finishes_one_active_context_and_event_then_stops_demand() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]);
        let (endpoint, gate) = event_gate::endpoint(probe.clone());
        run(&runtime, async {
            let service = endpoint.__managed_service().unwrap();
            let mut body = body(&endpoint, &service, profile).await;
            drop(next(&mut body).await.unwrap());
            until(&gate.entered, 1).await;
            let mut shutdown = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| std::task::Poll::Ready(shutdown.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            assert_eq!(endpoint.shutdown_outcome(), None);
            assert!(service.request().is_err());
            gate.release();
            let event = next(&mut body).await.unwrap();
            assert!(event.contains("\"value\":7"), "{event}");
            shutdown.await;
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
            assert!(next(&mut body).await.is_none());
            assert_eq!(count(&probe.counts.source_polls), 1);
            assert_eq!(count(&probe.counts.source_maps), 0);
            assert_eq!(probe.event_generations.lock().unwrap().len(), 1);
            assert_eq!(count(&probe.counts.nested_calls), 1);
            assert_eq!(count(&probe.counts.source_drops), 1);
            assert_eq!(count(&probe.counts.hook_calls), 1);
            assert_eq!(count(&probe.counts.completion_calls), 1);
        });
    }
}

#[test]
fn managed_graceful_expiry_revokes_emission_and_late_event_cannot_report_drained() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(7)]);
        let (endpoint, gate) = event_gate::endpoint(probe.clone());
        run(&runtime, async {
            let service = endpoint.__managed_service().unwrap();
            let mut body = body(&endpoint, &service, profile).await;
            drop(next(&mut body).await.unwrap());
            until(&gate.entered, 1).await;
            endpoint
                .shutdown_on(
                    async {},
                    ShutdownOptions::graceful_for(Duration::from_millis(10)).unwrap(),
                )
                .await;
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
            gate.release();
            assert!(matches!(
                poll_fn(|cx| body.poll_next(cx)).await,
                Some(Err(_))
            ));
            until(&probe.counts.source_drops, 1).await;
            assert_eq!(count(&probe.counts.source_maps), 0);
            assert_eq!(count(&probe.counts.nested_calls), 0);
            assert!(probe.event_generations.lock().unwrap().is_empty());
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        });
    }
}

#[test]
fn managed_expiry_never_adds_a_cleanup_wait_or_releases_a_blocked_physical_source() {
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let endpoint = session_endpoint(
        probe.clone(),
        SubscriptionLimits {
            max_active_subscriptions: 1,
            ..SubscriptionLimits::default()
        },
    );
    let blocked = probe.block_source_drop();
    run(&runtime, async {
        let service = endpoint.__managed_service().unwrap();
        let mut body = body(&endpoint, &service, SubscriptionTransport::ServerSentEvents).await;
        drop(next(&mut body).await.unwrap());
        until_pending_source(&probe).await;
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(10)).unwrap(),
            ),
        )
        .await
        .expect("the original deadline cannot acquire a fresh five-second cleanup wait");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        until(&probe.counts.source_drops, 1).await;
        drop(body);
        let pending = endpoint.__begin_http_subscription().unwrap();
        assert!(
            pending
                .activate(crate::subscription::bridge_resources::BridgeOperationKind::Subscription)
                .is_err(),
            "a blocked concrete source still owns its original active permit"
        );
        blocked.release();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let pending = endpoint.__begin_http_subscription().unwrap();
                if let Ok(active) = pending.activate(
                    crate::subscription::bridge_resources::BridgeOperationKind::Subscription,
                ) {
                    drop(active);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("physical join releases the original charged generation");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
    });
}

#[test]
fn managed_no_hook_last_owner_and_immediate_hook_request_cancellation() {
    let runtime = runtime();
    for immediate_hook in [false, true].into_iter().cycle().take(16) {
        let probe = State::new(vec![SourceStep::Pending]);
        let endpoint = session_endpoint(probe.clone(), SubscriptionLimits::default());
        run(&runtime, async {
            let service = endpoint.__managed_service().unwrap();
            let cloned = service.clone();
            let mut body = body(&endpoint, &service, SubscriptionTransport::ServerSentEvents).await;
            drop(next(&mut body).await.unwrap());
            until_pending_source(&probe).await;
            drop(service);
            assert_eq!(endpoint.shutdown_outcome(), None);
            if immediate_hook {
                endpoint
                    .shutdown_on(async {}, ShutdownOptions::immediate())
                    .await;
                assert!(cloned.request().is_err());
            }
            drop(cloned);
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::CancellationRequested)
            );
            until(&probe.counts.source_drops, 1).await;
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("last-owner or Immediate cancellation physically joins its SDK supervisor");
            assert!(next(&mut body).await.is_none());
            assert_eq!(count(&probe.counts.nested_calls), 0);
            assert_eq!(count(&probe.counts.hook_calls), 1);
            assert_eq!(count(&probe.counts.completion_calls), 1);
        });
    }
}
