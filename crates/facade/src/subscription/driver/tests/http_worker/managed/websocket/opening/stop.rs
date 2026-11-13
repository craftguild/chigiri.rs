//! Accepted stop wins pending receive, ACK start and ACK activation boundaries.

mod socket;

use super::*;
use crate::managed::Control;
use crate::subscription::driver::tests::websocket_session::socket::Handle;
use chigiri_http::{
    ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision, ConnectionInitError,
};
use socket::{Boundary, SignalSocket};

struct Authorize(Arc<AtomicUsize>);
impl ConnectionInitAuthorizer for Authorize {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(Ok(ConnectionInitDecision::Accept)))
    }
}

struct CountTimers(AtomicUsize);
static CLOCK: CountTimers = CountTimers(AtomicUsize::new(0));
impl DeadlineFactory for CountTimers {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if duration == SubscriptionLimits::default().heartbeat_interval {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        Box::pin(tokio::time::sleep(duration))
    }
}

#[test]
fn accepted_stop_precedes_opening_input_ack_start_and_activation() {
    let runtime = runtime();
    for boundary in [Boundary::Receive, Boundary::Ready, Boundary::Write] {
        for graceful in [false, true] {
            CLOCK.0.store(0, Ordering::SeqCst);
            let probe = State::new(Vec::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let errors = Arc::new(AtomicUsize::new(0));
            let signalled = Arc::new(AtomicUsize::new(0));
            let schema =
                ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
                    .build()
                    .unwrap();
            let endpoint = Endpoint::builder(schema)
                .context_factory(Factory(probe.clone()))
                .connection_init_authorizer(Authorize(calls.clone()))
                .inject(|components| {
                    components
                        .observability()
                        .error_observer(Errors(errors.clone()));
                })
                .build()
                .unwrap();
            run(&runtime, async {
                let (service, upgrade) = managed_socket::admit(&endpoint).await;
                let peer = Handle::default();
                peer.text(r#"{"type":"connection_init"}"#);
                let socket = SignalSocket::new(
                    peer.socket(),
                    endpoint.managed_control(),
                    boundary,
                    if graceful {
                        ShutdownOptions::graceful()
                    } else {
                        ShutdownOptions::immediate()
                    },
                    signalled.clone(),
                );
                endpoint
                    .__attach_managed_websocket(
                        upgrade,
                        Box::pin(std::future::ready(Ok(socket))),
                        &CLOCK,
                    )
                    .unwrap();
                until(&signalled, 1).await;
                let control = endpoint.managed_control();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while crate::subscription::worker::managed_supervisor_retained_for_test(
                        &control,
                    ) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("accepted native stop must join its original supervisor");
                assert_eq!(
                    endpoint.shutdown_outcome(),
                    Some(if graceful {
                        ShutdownOutcome::Drained
                    } else {
                        ShutdownOutcome::CancellationRequested
                    })
                );
                assert_eq!(
                    CLOCK.0.load(Ordering::SeqCst),
                    0,
                    "a stopped pending connection cannot construct an active heartbeat"
                );
                assert_eq!(
                    count(&calls),
                    usize::from(!matches!(boundary, Boundary::Receive))
                );
                if !matches!(boundary, Boundary::Write) {
                    assert!(
                        peer.messages().is_empty(),
                        "stop accepted before ACK start must suppress its native transfer"
                    );
                }
                assert_eq!(
                    count(&errors),
                    0,
                    "ordinary opening cancellation is not infrastructure failure"
                );
                assert_eq!(count(&probe.counts.factory_calls), 0);
                assert_eq!(count(&probe.counts.setup_calls), 0);
                assert_eq!(count(&probe.counts.completion_calls), 0);
                assert_eq!(endpoint.subscription_connection_counts_for_test().0, 0);
                assert!(service.request().is_err());
            });
        }
    }
}
