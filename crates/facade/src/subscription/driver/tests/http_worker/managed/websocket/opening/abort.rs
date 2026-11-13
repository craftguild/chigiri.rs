//! Supervisor cancellation retains its actual join and original native charge.

use super::blocking_socket::BlockingSocket;
use super::*;
use crate::subscription::driver::tests::websocket_session::socket::Handle;

#[test]
fn supervisor_abort_keeps_native_ownership_during_opening_ack_and_active_disposal() {
    let runtime = runtime();
    for phase in 0..3 {
        let probe = State::new(Vec::new());
        let release = Release(probe.block_scalar());
        let dropped = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
            .build()
            .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(Factory(probe.clone()))
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
            if phase != 0 {
                peer.text(r#"{"type":"connection_init"}"#);
            }
            if phase == 1 {
                peer.stall_flush();
            }
            let received = Arc::new(AtomicUsize::new(0));
            let socket = BlockingSocket {
                socket: peer.socket(),
                probe: probe.clone(),
                dropped: dropped.clone(),
                received: received.clone(),
            };
            endpoint
                .__attach_managed_websocket(upgrade, Box::pin(async move { Ok(socket) }), &TIMERS)
                .unwrap();
            until(&received, 1).await;
            if phase != 0 {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while peer.messages().is_empty() || (phase == 2 && peer.has_current_frame()) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the fixture must reach the physical acknowledgement boundary");
                assert_eq!(peer.messages(), [r#"{"type":"connection_ack"}"#]);
            }
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while !crate::subscription::worker::abort_managed_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            until(&dropped, 1).await;
            assert!(release.0.wait_entered(Duration::from_secs(2)));
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
            assert!(crate::subscription::worker::managed_supervisor_retained_for_test(&control));
            assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
            assert!(service.request().is_err());
            tokio::time::timeout(
                Duration::from_secs(1),
                endpoint.shutdown_on(async {}, ShutdownOptions::graceful()),
            )
            .await
            .expect("infrastructure failure must complete the native stop hook");
            assert_eq!(count(&errors), 0);
            assert_eq!(count(&probe.counts.factory_calls), 0);
            assert_eq!(count(&probe.counts.setup_calls), 0);
            release.0.release();
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.subscription_connection_counts_for_test().0 != 0
                    || crate::subscription::worker::managed_supervisor_retained_for_test(&control)
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("original supervisor JoinReady follows successful socket destruction");
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
        });
    }
}
