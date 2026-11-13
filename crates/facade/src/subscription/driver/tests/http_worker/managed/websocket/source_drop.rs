//! The original source permit survives a failed concrete source destructor.

use std::process::Command;

use super::super::super::terminal::application::panicking_source_schema;
use super::*;

const CHILD: &str = "CHIGIRI_MANAGED_WS_SOURCE_DROP_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::source_drop::failed_source_destruction_retains_its_original_active_and_event_owners";

#[test]
fn failed_source_destruction_retains_its_original_active_and_event_owners() {
    let Ok(mode) = std::env::var(CHILD) else {
        for mode in ["pending", "complete"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .arg(TEST)
                .arg("--exact")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(CHILD, mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned {mode} source child failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let runtime = runtime();
    let probe = State::new(Vec::new());
    let errors = Arc::new(AtomicUsize::new(0));
    let endpoint = Endpoint::builder(panicking_source_schema(probe.clone(), mode == "complete"))
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
        managed
            .peer
            .subscribe("source", Scenario::SourceContext.document());
        until(&probe.counts.source_polls, 1).await;
        tokio::time::timeout(
            Duration::from_secs(1),
            endpoint.shutdown_on(async {}, ShutdownOptions::graceful()),
        )
        .await
        .expect("a classified failure hook cannot wait for process recovery");
        until(&errors, 1).await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::process_poisoned_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| { panic!("the original source worker must reach poisoned native JoinReady; error={error}, {}", crate::subscription::worker::process_diagnostic_for_test()) });
        assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        assert_eq!(endpoint.subscription_physical_counts_for_test().0, 1);
        assert!(
            endpoint.subscription_connection_counts_for_test().1 > 0,
            "the original pre-demand event reservation remains charged"
        );
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(count(&probe.counts.completion_drops), 1);
        assert_eq!(count(&errors), 1);
        assert!(managed.service.request().is_err());
    });
}
