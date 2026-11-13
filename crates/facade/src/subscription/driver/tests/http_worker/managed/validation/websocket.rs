//! Managed upgrade/session ownership must observe late validation failure itself.

use super::super::super::super::harness::managed_socket;
use super::*;

#[test]
fn managed_websocket_late_validation_panic_fails_without_an_application_driver() {
    failure(true);
}

#[test]
fn managed_websocket_validation_panic_closes_admission_without_a_shutdown_signal() {
    failure(false);
}

fn failure(stop_first: bool) {
    let runtime = runtime();
    let probe = State::new(Vec::new());
    let release = Release(probe.block_scalar());
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
        let managed = managed_socket::open(&endpoint).await;
        let service = &managed.service;
        let socket = &managed.peer;
        socket.text(r#"{"id":"pending","type":"subscribe","payload":{"query":"subscription Named($value: Probe!) { events(limit: 2, probe: $value) { value } }","operationName":"Named","variables":{"value":"panic-after-release"}}}"#);
        tokio::time::timeout(Duration::from_secs(2), async {
            while count(&probe.counts.scalar_calls) != 1 {
                tokio::task::yield_now().await;
            }
        }).await.unwrap_or_else(|_| panic!("managed validation not reached: frames={:?}, closes={:?}, phase={:?}, outcome={:?}", socket.messages(), socket.closes(), endpoint.managed_control().phase(), endpoint.shutdown_outcome()));
        assert!(
            socket
                .messages()
                .iter()
                .any(|frame| frame == r#"{"type":"connection_ack"}"#)
        );
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
        until(&errors, 1).await;
        shutdown
            .unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&errors), 1);
        assert_eq!(count(&probe.counts.scalar_calls), 1);
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert!(service.request().is_err());
        let control = endpoint.managed_control();
        tokio::time::timeout(Duration::from_secs(2), async {
            while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("failed managed WebSocket supervision must physically join");
        assert_eq!(endpoint.subscription_pending_probe_for_test()(), (0, 0));
        for frame in socket.messages() {
            assert!(!frame.contains("private validation"));
            assert!(!frame.contains(r#""type":"next""#));
        }
    });
}
