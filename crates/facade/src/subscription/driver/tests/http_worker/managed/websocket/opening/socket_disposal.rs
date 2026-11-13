//! The admitted connection outlives native socket destruction in every phase.

use super::blocking_socket::BlockingSocket;
use super::*;
use crate::subscription::driver::tests::websocket_session::socket::Handle;

#[test]
fn blocked_opening_socket_disposal_retains_the_connection_after_expiry() {
    exercise(false);
}

#[test]
fn blocked_active_socket_disposal_retains_the_connection_after_expiry() {
    exercise(true);
}

fn exercise(active: bool) {
    let runtime = runtime();
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
        if active {
            peer.text(r#"{"type":"connection_init"}"#);
        }
        let socket = BlockingSocket {
            socket: peer.socket(),
            probe: probe.clone(),
            dropped: dropped.clone(),
            received: Arc::new(AtomicUsize::new(0)),
        };
        endpoint
            .__attach_managed_websocket(upgrade, Box::pin(async move { Ok(socket) }), &TIMERS)
            .unwrap();
        if active {
            tokio::time::timeout(Duration::from_secs(2), async {
                while peer.messages().is_empty() || peer.has_current_frame() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the fixture must reach an acknowledged active connection");
            assert_eq!(peer.messages(), [r#"{"type":"connection_ack"}"#]);
        }
        peer.text("invalid JSON");
        until(&dropped, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            ),
        )
        .await
        .expect("socket disposal cannot extend the scoped wait");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(
            endpoint.subscription_connection_counts_for_test().0,
            1,
            "the same admitted connection must outlive physical socket destruction"
        );
        assert_eq!(count(&errors), 0);
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert!(service.request().is_err());
        release.0.release();
        tokio::time::timeout(Duration::from_secs(2), async {
            while endpoint.subscription_connection_counts_for_test().0 != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("successful physical socket destruction releases its original connection");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
    });
}
