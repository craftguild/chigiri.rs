//! A committed managed stream must surface task failure without clean EOF.

use std::sync::Arc;

use chigiri_http::RequestPhase;
use chigiri_runtime::{ErrorEvent, ErrorObserver, ExecutableSchema};

use super::super::super::descriptor::SCHEMA;
use super::super::super::dispatch::Dispatch;
use super::super::super::harness::Hook;
use super::super::super::state::{EventMode, FactoryMode, HookMode, SetupMode};
use super::*;

struct Errors(Arc<AtomicUsize>);

impl ErrorObserver for Errors {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn event_context_panic_is_observed_once_and_breaks_the_committed_body() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::modes(
            vec![SourceStep::Event(7)],
            SetupMode::Ready,
            EventMode::Object,
            FactoryMode::Ready,
            FactoryMode::Panic,
            HookMode::Ready,
        );
        let (factory, gate) = event_gate::factory(probe.clone());
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
            .build()
            .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(factory)
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
            let mut body = body(&endpoint, &service, profile).await;
            drop(next(&mut body).await.unwrap());
            until(&gate.entered, 1).await;
            assert_eq!(count(&errors), 0);
            assert_eq!(count(&probe.counts.factory_calls), 1);
            // The event context may fail only after the real response commit.
            gate.release();
            until(&errors, 1).await;
            until(&probe.counts.source_drops, 1).await;
            assert!(matches!(
                poll_fn(|cx| body.poll_next(cx)).await,
                Some(Err(_))
            ));
            endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
                .await;
            assert_eq!(
                endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
            assert!(service.request().is_err());
            assert_eq!(count(&errors), 1);
            assert_eq!(count(&probe.counts.factory_calls), 2);
            assert_eq!(count(&probe.counts.completion_calls), 1);
            assert_eq!(count(&probe.counts.hook_calls), 1);
            assert_eq!(count(&probe.counts.nested_calls), 0);
            assert!(matches!(
                probe.phases.lock().unwrap().as_slice(),
                [
                    RequestPhase::SubscriptionSetup { .. },
                    RequestPhase::SubscriptionEvent { .. }
                ]
            ));
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("failed managed source must physically join");
            let (active, bytes) = endpoint.subscription_physical_counts_for_test();
            assert_eq!(active, 1);
            assert!(bytes > 0, "the native body still owns its initial bytes");
            drop(body);
            assert_eq!(endpoint.subscription_physical_counts_for_test(), (0, 0));
        });
    }
}
