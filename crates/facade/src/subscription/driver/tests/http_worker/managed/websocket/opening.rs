//! Native upgrade disposal must retain the already charged connection owner.

mod abort;
mod blocking_socket;
mod failures;
mod registration;
mod socket_disposal;
mod socket_faults;
mod stop;

use super::*;
use crate::__streaming::WebSocketIoError;
use crate::subscription::driver::tests::websocket_session::socket::Socket;

struct NativeUpgrade {
    polled: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    probe: Arc<State>,
}
impl Future for NativeUpgrade {
    type Output = Result<Socket, WebSocketIoError>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polled.store(1, Ordering::SeqCst);
        Poll::Pending
    }
}
impl Drop for NativeUpgrade {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
        self.probe.wait_in_scalar();
    }
}

#[test]
fn blocked_native_upgrade_destruction_keeps_its_original_connection_after_scoped_expiry() {
    let runtime = runtime();
    let probe = State::new(Vec::new());
    let release = Release(probe.block_scalar());
    let polled = Arc::new(AtomicUsize::new(0));
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
        endpoint
            .__attach_managed_websocket(
                upgrade,
                Box::pin(NativeUpgrade {
                    polled: polled.clone(),
                    dropped: dropped.clone(),
                    probe: probe.clone(),
                }),
                &TIMERS,
            )
            .unwrap();
        until(&polled, 1).await;
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            ),
        )
        .await
        .expect("native upgrade disposal cannot add another scoped cleanup wait");
        until(&dropped, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(
            endpoint.subscription_connection_counts_for_test().0,
            1,
            "the original admitted connection must outlive the native future destructor"
        );
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&errors), 0);
        assert!(service.request().is_err());
        release.0.release();
        tokio::time::timeout(Duration::from_secs(2), async {
            while endpoint.subscription_connection_counts_for_test().0 != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("successful native future disposal releases the original connection");
        assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Expired));
    });
}

#[test]
fn immediate_and_last_service_drop_cancel_native_conversion_without_claiming_drain() {
    let runtime = runtime();
    for immediate in [true, false] {
        let probe = State::new(Vec::new());
        let release = Release(probe.block_scalar());
        let polled = Arc::new(AtomicUsize::new(0));
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
            // Native attachment may occur outside an entered runtime. It must
            // use the activation binding, without creating another runtime.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        endpoint.__attach_managed_websocket(
                            upgrade,
                            Box::pin(NativeUpgrade {
                                polled: polled.clone(),
                                dropped: dropped.clone(),
                                probe: probe.clone(),
                            }),
                            &TIMERS,
                        )
                    })
                    .join()
                    .unwrap()
                    .unwrap();
            });
            until(&polled, 1).await;
            if immediate {
                tokio::time::timeout(
                    Duration::from_secs(1),
                    endpoint.shutdown_on(async {}, ShutdownOptions::immediate()),
                )
                .await
                .expect("Immediate requests cancellation without waiting for disposal");
            } else {
                let last = service.clone();
                drop(service);
                drop(last.request().unwrap());
                assert_eq!(
                    count(&dropped),
                    0,
                    "a remaining service lease keeps the conversion owned"
                );
                drop(last);
            }
            until(&dropped, 1).await;
            assert!(release.0.wait_entered(Duration::from_secs(2)));
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::CancellationRequested)
            );
            assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
            assert_eq!(count(&errors), 0);
            assert_eq!(count(&probe.counts.factory_calls), 0);
            release.0.release();
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.subscription_connection_counts_for_test().0 != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("original native completion releases its connection after cancellation");
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::CancellationRequested)
            );
        });
    }
}
