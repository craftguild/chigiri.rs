//! Actual managed validation failure after accepted endpoint stop.

mod disposal;
mod websocket;

use std::sync::Arc;

use chigiri_runtime::{ErrorEvent, ErrorObserver, ExecutableSchema};

use super::super::super::descriptor::SCHEMA;
use super::super::super::dispatch::Dispatch;
use super::super::super::harness::{Factory, Hook};
use super::super::super::state::ScalarGate;
use super::*;

const COERCION: &[u8] = br#"{"query":"subscription Named($value: Probe!) { events(limit: 2, probe: $value) { value } }","operationName":"Named","variables":{"value":"panic-after-release"}}"#;

struct Errors(Arc<AtomicUsize>);
impl ErrorObserver for Errors {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Release(Arc<ScalarGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn invoke(
    endpoint: &Endpoint<AppContext>,
    service: &ManagedService,
    profile: SubscriptionTransport,
    payload: &[u8],
) -> BoxFuture<'static, ManagedHttpResponse> {
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
    endpoint.__handle_managed_http(
        service.request().unwrap(),
        TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata),
        BufferedBody::new(payload.to_vec()),
        WebSocketHttpVersion::Http11,
        &TIMERS,
    )
}

#[test]
fn validation_panic_after_stop_cannot_be_reported_as_drained() {
    failure(true);
}

#[test]
fn ordinary_validation_panic_closes_admission_and_returns_safe_500() {
    failure(false);
}

fn failure(stop_first: bool) {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let gate = probe.block_scalar();
        let release = Release(gate);
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
            .build()
            .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(Factory(probe.clone()))
            .response_hook(Hook(probe.clone()))
            .inject(|components| {
                components
                    .observability()
                    .error_observer(Errors(errors.clone()));
            })
            .build()
            .unwrap();
        run(&runtime, async {
            let service = endpoint.__managed_service().unwrap();
            let response = invoke(&endpoint, &service, profile, COERCION);
            until(&probe.counts.scalar_calls, 1).await;
            let shutdown = if stop_first {
                let mut hook = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
                assert!(
                    poll_fn(|cx| std::task::Poll::Ready(hook.as_mut().poll(cx)))
                        .await
                        .is_pending()
                );
                Some(hook)
            } else {
                None
            };
            release.0.release();
            let response = response.await;
            if stop_first {
                assert!(matches!(
                    response,
                    ManagedHttpResponse::Empty { status: 500 | 503 }
                ));
            } else {
                assert!(matches!(
                    response,
                    ManagedHttpResponse::Empty { status: 500 }
                ));
            }
            shutdown
                .unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
                .await;
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
            until(&errors, 1).await;
            assert_eq!(count(&errors), 1);
            assert_eq!(count(&probe.counts.factory_calls), 0);
            assert_eq!(count(&probe.counts.setup_calls), 0);
            assert_eq!(count(&probe.counts.hook_calls), 0);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            assert!(service.request().is_err());
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("failed validation supervision must physically join");
            assert_eq!(endpoint.subscription_pending_probe_for_test()(), (0, 0));
        });
    }
}
