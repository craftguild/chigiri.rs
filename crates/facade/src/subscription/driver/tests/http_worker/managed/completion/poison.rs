//! Irreversible source-task disposal failure is isolated in an owned child.

use std::process::Command;

use super::*;

const CHILD: &str = "CHIGIRI_MANAGED_SUBSCRIPTION_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::completion::poison::failed_disposal_keeps_the_original_active_permit_and_initial_bytes";

#[test]
fn failed_disposal_keeps_the_original_active_permit_and_initial_bytes() {
    let Some(case) = std::env::var_os(CHILD) else {
        for mode in ["payload", "token", "both"] {
            for profile in ["sse", "multipart"] {
                for order in ["stop-first", "complete-first"] {
                    let output = Command::new(std::env::current_exe().unwrap())
                        .arg(TEST)
                        .arg("--exact")
                        .arg("--nocapture")
                        .arg("--test-threads=1")
                        .env(CHILD, format!("{mode}:{profile}:{order}"))
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "owned poison child {mode}:{profile}:{order} failed: {}\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
        return;
    };
    let case = case.to_str().unwrap();
    let mut parts = case.split(':');
    let mode = parts.next().unwrap();
    let profile = parts.next().unwrap();
    let order = match parts.next().unwrap() {
        "stop-first" => ShutdownOrder::StopBeforeCompletion,
        "complete-first" => ShutdownOrder::CompleteBeforeStop,
        _ => unreachable!(),
    };
    assert!(parts.next().is_none());
    let mode = match mode {
        "payload" => FailureMode::Payload,
        "token" => FailureMode::Token,
        "both" => FailureMode::Both,
        _ => unreachable!(),
    };
    let profile = match profile {
        "sse" => SubscriptionTransport::ServerSentEvents,
        "multipart" => SubscriptionTransport::Multipart,
        _ => unreachable!(),
    };
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (endpoint, signals, mut release) = fixture(probe.clone(), mode);
    run(&runtime, async {
        let service = endpoint.__managed_service().unwrap();
        fail_initial(
            &endpoint,
            &service,
            profile,
            &signals,
            &probe,
            &mut release,
            order,
        )
        .await;
        // The process selects this poisoned record only after actual native
        // JoinReady. The original Endpoint is deliberately retained alive.
        tokio::time::timeout(Duration::from_secs(2), async {
            while !crate::subscription::worker::process_poisoned_for_test() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "native join must preserve the reachable poisoned record; case={case}, error={error}, {}",
                crate::subscription::worker::process_diagnostic_for_test()
            )
        });
        let (active, bytes) = endpoint.subscription_physical_counts_for_test();
        assert_eq!(
            active, 1,
            "failed destruction cannot release the original active permit"
        );
        assert!(
            bytes >= crate::subscription::control::MAX_INITIAL_RESPONSE_BYTES,
            "original initial byte guards must remain charged"
        );
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        endpoint
            .shutdown_on(async {}, ShutdownOptions::graceful())
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&signals.errors), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        drop(service);
    });
}
