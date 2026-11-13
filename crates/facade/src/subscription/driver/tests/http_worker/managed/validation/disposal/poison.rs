//! A failed last-owner destructor poisons only an owned child test process.

use std::process::Command;

use super::*;

const CHILD: &str = "CHIGIRI_VALIDATION_DISPOSAL_POISON_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::validation::disposal::poison::failed_graph_destruction_retains_original_pending_owners_after_native_join";

#[test]
fn failed_graph_destruction_retains_original_pending_owners_after_native_join() {
    let Some(profile) = std::env::var_os(CHILD) else {
        for profile in ["sse", "multipart"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .arg(TEST)
                .arg("--exact")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env(CHILD, profile)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned validation child failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let profile = match profile.to_str().unwrap() {
        "sse" => SubscriptionTransport::ServerSentEvents,
        "multipart" => SubscriptionTransport::Multipart,
        _ => unreachable!(),
    };
    exercise(profile, true);
}
