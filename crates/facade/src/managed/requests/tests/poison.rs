//! Irrecoverable disposal must keep its original physical request charged.

use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::support::{Harness, Mode, invoke, runtime, until};
use crate::{ShutdownOptions, ShutdownOutcome};

const CHILD: &str = "CHIGIRI_MANAGED_HTTP_POISON_CHILD";

#[test]
fn failed_completion_panic_disposal_retains_the_original_physical_owner() {
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("managed::requests::tests::poison::failed_completion_panic_disposal_retains_the_original_physical_owner")
            .arg("--exact").arg("--nocapture").arg("--test-threads=1")
            .env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "isolated poison fixture failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        *harness.probe.mode.lock().unwrap() = Mode::CompletionPayloadDropPanic;
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "mutation { ping }").await;
        assert!(matches!(
            response,
            crate::managed::ManagedHttpResponse::Empty { status: 500 }
        ));
        until(|| {
            harness.probe.completion_drops.load(Ordering::SeqCst) == 1
                && !harness.probe.errors.lock().unwrap().is_empty()
        })
        .await;
        // This isolated fixture has one failed request. The reaper selects its
        // poisoned record only after native JoinReady, not a task-side flag.
        until(crate::subscription::worker::process_poisoned_for_test).await;
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert!(
            !harness.endpoint.managed_control().requests_idle(),
            "failed disposal cannot release its physical request count"
        );
        assert!(service.request().is_err());
        harness
            .endpoint
            .shutdown_on(async {}, ShutdownOptions::graceful())
            .await;
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(harness.probe.completions.lock().unwrap().len(), 1);
        assert_eq!(harness.probe.errors.lock().unwrap().len(), 1);
        drop(service);
    });
}
