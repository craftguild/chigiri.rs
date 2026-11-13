//! Failed active-task disposal is isolated in owned and joined test children.

use std::process::Command;

use super::*;

const CHILD: &str = "CHIGIRI_MANAGED_WS_ACTIVE_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::poison::failed_payload_disposal_retains_original_connection_and_operation_owners";
const FUTURE_TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::poison::failed_context_future_disposal_retains_original_active_owners";

#[test]
fn failed_payload_disposal_retains_original_connection_and_operation_owners() {
    cases(TEST, Failure::Payload);
}

#[test]
fn failed_context_future_disposal_retains_original_active_owners() {
    cases(FUTURE_TEST, Failure::FutureDrop);
}

fn cases(test: &str, failure: Failure) {
    let Some(scenario) = std::env::var_os(CHILD) else {
        for scenario in ["source", "query", "mutation", "event"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .arg(test)
                .arg("--exact")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(CHILD, scenario)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned active-task child failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let scenario = match scenario.to_str().unwrap() {
        "query" => Scenario::Query,
        "mutation" => Scenario::Mutation,
        "source" => Scenario::SourceContext,
        "event" => Scenario::SourceEvent,
        _ => unreachable!(),
    };
    exercise(&runtime(), scenario, true, failure);
}
