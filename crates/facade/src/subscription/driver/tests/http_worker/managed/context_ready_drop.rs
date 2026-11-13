//! Failed successful HTTP source-context future Drop keeps initial completion.

use std::process::Command;
use std::sync::Arc;

use chigiri_http::ResponseCompletionOutcome;
use chigiri_runtime::{ErrorEvent, ErrorObserver, ExecutableSchema};

use super::super::super::descriptor::SCHEMA;
use super::super::super::dispatch::Dispatch;
use super::super::super::harness::Hook;
use super::websocket::ready_drop::source_factory;
use super::*;

const CHILD: &str = "CHIGIRI_HTTP_SOURCE_READY_CONTEXT_DROP_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::context_ready_drop::ready_context_future_failure_keeps_initial_completion_and_original_source_owners";

struct Errors(Arc<AtomicUsize>);
impl ErrorObserver for Errors {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Release(Arc<super::super::super::state::ScalarGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
fn ready_context_future_failure_keeps_initial_completion_and_original_source_owners() {
    let Ok(case) = std::env::var(CHILD) else {
        for profile in ["sse", "multipart"] {
            for stop_first in [false, true] {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(CHILD, format!("{profile}:{stop_first}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "owned source {profile}:{stop_first} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (profile, stop_first) = case.split_once(':').unwrap();
    let profile = if profile == "sse" {
        SubscriptionTransport::ServerSentEvents
    } else {
        SubscriptionTransport::Multipart
    };
    let probe = State::new(Vec::new());
    let release = Release(probe.block_scalar());
    let errors = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(source_factory(probe.clone()))
        .response_hook(Hook(probe.clone()))
        .inject(|components| {
            components
                .observability()
                .error_observer(Errors(errors.clone()));
        })
        .build()
        .unwrap();
    run(&runtime(), async {
        let service = endpoint.__managed_service().unwrap();
        let response = super::completion::invoke(&endpoint, &service, profile);
        until(&probe.counts.factory_calls, 1).await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        let hook = if stop_first == "true" {
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
        until(&probe.counts.completion_drops, 1).await;
        hook.unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        assert!(matches!(
            response.await,
            ManagedHttpResponse::Empty { status: 500 | 503 }
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::process_poisoned_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "the original source worker must reach poisoned native JoinReady; case={case}, error={error}, {}",
                crate::subscription::worker::process_diagnostic_for_test()
            )
        });
        assert_eq!(count(&probe.counts.factory_future_drops), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(
            *probe.completion_outcomes.lock().unwrap(),
            [ResponseCompletionOutcome::Failed]
        );
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        assert_eq!(count(&errors), 1);
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        let (active, bytes) = endpoint.subscription_physical_counts_for_test();
        assert_eq!(active, 1);
        assert!(bytes >= crate::subscription::control::MAX_INITIAL_RESPONSE_BYTES);
        assert!(service.request().is_err());
    });
}
