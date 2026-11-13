use chigiri_testkit::{AdapterScenario, PendingDropProbe};
use std::path::Path;
use std::process::Command;

#[test]
fn adapter_provider_future_is_available_to_consumers() {
    let (probe, state) = PendingDropProbe::new();
    drop(probe);
    assert!(state.was_dropped());
    assert_eq!(AdapterScenario::PhaseOrder, AdapterScenario::PhaseOrder);
}

#[test]
fn bridges_expose_no_listener_route_or_shutdown() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = root.join("tests/fixtures/bridge-negative-surface/Cargo.toml");
    for binary in [
        "hyper-server",
        "hyper-listener",
        "hyper-serve",
        "hyper-shutdown",
        "axum-router",
        "axum-route",
        "axum-bind",
        "axum-shutdown",
    ] {
        let output = Command::new(env!("CARGO"))
            .args(["check", "--locked", "--offline", "--manifest-path"])
            .arg(&manifest)
            .args(["--bin", binary, "--target-dir"])
            .arg(root.join("target/bridge-negative-surface"))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "The forbidden bridge API is open."
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unresolved import"),
            "the test binary path must be available"
        );
    }
}
