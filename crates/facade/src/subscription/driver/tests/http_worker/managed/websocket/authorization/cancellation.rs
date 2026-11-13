//! Pending authorization disposal stays in its task through every stop mode.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::*;

struct PendingAuthorization {
    polled: AtomicUsize,
    dropped: AtomicUsize,
    owner: Mutex<Option<tokio::task::Id>>,
    same_owner: AtomicBool,
    gate: Arc<State>,
}

struct Authorize(Arc<PendingAuthorization>);
impl ConnectionInitAuthorizer for Authorize {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        Box::pin(Waiting(Arc::clone(&self.0)))
    }
}

struct Waiting(Arc<PendingAuthorization>);
impl Future for Waiting {
    type Output = Result<ConnectionInitDecision, ConnectionInitError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        *self.0.owner.lock().unwrap() = tokio::task::try_id();
        self.0.polled.store(1, Ordering::SeqCst);
        Poll::Pending
    }
}
impl Drop for Waiting {
    fn drop(&mut self) {
        let owner = *self.0.owner.lock().unwrap();
        self.0.same_owner.store(
            owner.is_some() && owner == tokio::task::try_id(),
            Ordering::SeqCst,
        );
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
        self.0.gate.wait_in_scalar();
    }
}

#[test]
fn pending_authorization_keeps_original_ownership_for_hooks_and_last_service_drop() {
    let runtime = runtime();
    for mode in 0..3 {
        let app = State::new(Vec::new());
        let gate = State::new(Vec::new());
        let release = Release(gate.block_scalar());
        let auth = Arc::new(PendingAuthorization {
            polled: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            owner: Mutex::new(None),
            same_owner: AtomicBool::new(false),
            gate,
        });
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(app.clone()))
            .build()
            .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(Factory(app.clone()))
            .connection_init_authorizer(Authorize(Arc::clone(&auth)))
            .inject(|components| {
                components
                    .observability()
                    .error_observer(Errors(errors.clone()));
            })
            .build()
            .unwrap();
        run(&runtime, async {
            let managed = managed_socket::begin(&endpoint).await;
            until(&auth.polled, 1).await;
            let last = managed.service.clone();
            drop(managed.service);
            drop(last.request().unwrap());
            assert_eq!(count(&auth.dropped), 0);
            let expected = if mode == 2 {
                drop(last);
                ShutdownOutcome::CancellationRequested
            } else {
                let options = if mode == 0 {
                    ShutdownOptions::immediate()
                } else {
                    ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap()
                };
                tokio::time::timeout(
                    Duration::from_secs(1),
                    endpoint.shutdown_on(async {}, options),
                )
                .await
                .expect("pending authorization disposal cannot extend SDK waiting");
                assert!(last.request().is_err());
                drop(last);
                if mode == 0 {
                    ShutdownOutcome::CancellationRequested
                } else {
                    ShutdownOutcome::Expired
                }
            };
            until(&auth.dropped, 1).await;
            assert!(release.0.wait_entered(Duration::from_secs(2)));
            assert!(auth.same_owner.load(Ordering::SeqCst));
            assert_eq!(endpoint.shutdown_outcome(), Some(expected));
            assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
            assert!(managed.peer.messages().is_empty());
            assert_eq!(count(&errors), 0);
            assert_eq!(count(&app.counts.factory_calls), 0);
            assert_eq!(count(&app.counts.setup_calls), 0);
            assert_eq!(count(&app.counts.completion_calls), 0);
            release.0.release();
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while endpoint.subscription_connection_counts_for_test().0 != 0
                    || crate::subscription::worker::managed_supervisor_retained_for_test(&control)
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("successful cancellation must physically join the original owners");
            assert_eq!(count(&auth.dropped), 1);
            assert_eq!(endpoint.shutdown_outcome(), Some(expected));
        });
    }
}
