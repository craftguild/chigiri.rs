//! Actual managed initialization must retain task failure after outside stop.

mod cancellation;
mod disposal;

use chigiri_http::{
    ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision, ConnectionInitError,
};

use super::*;

struct PanickingAuthorizer(Arc<State>, Arc<AtomicUsize>);
impl ConnectionInitAuthorizer for PanickingAuthorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        Box::pin(async move {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.wait_in_scalar();
            panic!("private connection initialization panic");
        })
    }
}

#[test]
fn authorization_panic_is_scoped_infrastructure_failure_before_and_after_stop() {
    let runtime = runtime();
    for stop_first in [false, true] {
        let probe = State::new(Vec::new());
        let release = Release(probe.block_scalar());
        let calls = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
            .build()
            .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(Factory(probe.clone()))
            .connection_init_authorizer(PanickingAuthorizer(probe.clone(), calls.clone()))
            .inject(|components| {
                components
                    .observability()
                    .error_observer(Errors(errors.clone()));
            })
            .build()
            .unwrap();
        run(&runtime, async {
            let managed = managed_socket::begin(&endpoint).await;
            until(&calls, 1).await;
            assert!(release.0.wait_entered(Duration::from_secs(2)));
            let hook = if stop_first {
                let mut hook = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
                assert!(
                    poll_fn(|cx| Poll::Ready(hook.as_mut().poll(cx)))
                        .await
                        .is_pending()
                );
                Some(hook)
            } else {
                None
            };
            release.0.release();
            until(&errors, 1).await;
            hook.unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
                .await;
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
            assert_eq!(count(&calls), 1);
            assert_eq!(count(&errors), 1);
            assert_eq!(count(&probe.counts.factory_calls), 0);
            assert_eq!(count(&probe.counts.setup_calls), 0);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            assert!(managed.service.request().is_err());
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("recoverable authorization failure must physically join");
            assert_eq!(endpoint.subscription_connection_counts_for_test(), (0, 0));
            assert!(
                managed.peer.messages().is_empty(),
                "failed authorization cannot acknowledge or emit results"
            );
        });
    }
}
