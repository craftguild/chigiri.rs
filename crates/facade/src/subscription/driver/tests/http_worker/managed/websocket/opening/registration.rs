//! Stop and native-upgrade receipt cannot publish an opening after closure.

use std::sync::Barrier;

use super::blocking_socket::BlockingSocket;
use super::*;
use crate::subscription::driver::tests::websocket_session::socket::Handle;

#[test]
fn stop_before_attachment_or_native_receipt_prevents_opening_and_releases_original_owners() {
    let runtime = runtime();
    for graceful in [false, true] {
        for boundary in 0..10 {
            let probe = State::new(Vec::new());
            let errors = Arc::new(AtomicUsize::new(0));
            let schema =
                ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
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
                peer.text(r#"{"type":"connection_init"}"#);
                let received = Arc::new(AtomicUsize::new(0));
                let dropped = Arc::new(AtomicUsize::new(0));
                let polled = Arc::new(AtomicUsize::new(0));
                let socket = BlockingSocket {
                    socket: peer.socket(),
                    probe: probe.clone(),
                    dropped: dropped.clone(),
                    received: received.clone(),
                };
                let options = if graceful {
                    ShutdownOptions::graceful()
                } else {
                    ShutdownOptions::immediate()
                };
                let control = endpoint.managed_control();
                let conversion_control = control.clone();
                let conversion_polled = polled.clone();
                let conversion = Box::pin(async move {
                    conversion_polled.store(1, Ordering::SeqCst);
                    conversion_control.begin(options, tokio::time::Instant::now());
                    Ok(socket)
                });
                let attached = if boundary == 0 {
                    control.begin(options, tokio::time::Instant::now());
                    endpoint.__attach_managed_websocket(upgrade, conversion, &TIMERS)
                } else if boundary == 1 {
                    endpoint.__attach_managed_websocket(upgrade, conversion, &TIMERS)
                } else {
                    let barrier = Barrier::new(2);
                    std::thread::scope(|scope| {
                        let attach = scope.spawn(|| {
                            barrier.wait();
                            endpoint.__attach_managed_websocket(upgrade, conversion, &TIMERS)
                        });
                        barrier.wait();
                        control.begin(options, tokio::time::Instant::now());
                        attach.join().unwrap()
                    })
                };
                if let Err(error) = attached {
                    assert!(matches!(
                        error,
                        crate::ManagedServiceError::Closed
                            | crate::ManagedServiceError::Infrastructure
                    ));
                }
                tokio::time::timeout(Duration::from_secs(2), async {
                    while endpoint.subscription_connection_counts_for_test().0 != 0
                        || crate::subscription::worker::managed_supervisor_retained_for_test(
                            &control,
                        )
                    {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("a closed native handoff must dispose and join its charged owners");
                if boundary == 0 {
                    assert_eq!(count(&polled), 0);
                } else if boundary == 1 {
                    assert_eq!(count(&polled), 1);
                }
                assert_eq!(count(&dropped), 1);
                assert_eq!(count(&received), 0);
                assert!(peer.messages().is_empty());
                assert_eq!(count(&errors), 0);
                assert_eq!(count(&probe.counts.factory_calls), 0);
                assert_eq!(count(&probe.counts.setup_calls), 0);
                assert_eq!(count(&probe.counts.completion_calls), 0);
                assert!(service.request().is_err());
                assert_eq!(
                    endpoint.shutdown_outcome(),
                    Some(if graceful {
                        ShutdownOutcome::Drained
                    } else {
                        ShutdownOutcome::CancellationRequested
                    })
                );
            });
        }
    }
}
