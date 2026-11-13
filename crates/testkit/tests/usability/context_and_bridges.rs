//! Verify that endpoint bridges preserve the context typestate boundary.

use std::path::Path;
use std::process::Command;

use chigiri::spi::{ApplicationMetadata, RequestHeader};

use super::support::{NeverDeadline, block_on, endpoint, execute};

#[test]
fn api_05_context_typestate_compiles_and_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = root.join("tests/fixtures/usability-negative-surface/Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args(["check", "--locked", "--offline", "--manifest-path"])
        .arg(manifest)
        .args(["--bin", "context-missing", "--target-dir"])
        .arg(root.join("target/usability-negative-surface"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("no method named `build`"), "{stderr}");
    assert!(endpoint().configuration().safety_overrides().is_empty());
}

#[test]
fn api_08_all_inputs_use_the_same_endpoint_and_defaults() {
    let endpoint = endpoint();
    let digest = endpoint.schema_info().schema_digest();
    let direct = execute(&endpoint, "{ greeting }");
    assert!(direct.errors.is_empty());

    let headers = [
        RequestHeader::new("accept", b"application/graphql-response+json"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = chigiri::TransportRequest::new(
        "GET",
        "/graphql",
        Some("query=%7B%20greeting%20%7D"),
        &headers,
        None,
        &metadata,
    );
    let neutral =
        block_on(endpoint.handle(request, chigiri_http::BufferedBody::empty(), &NeverDeadline));
    assert_eq!(neutral.status, 200);

    let _axum = chigiri_axum::handler(endpoint.clone());
    let _hyper = chigiri_hyper::service(endpoint.clone());
    assert_eq!(endpoint.schema_info().schema_digest(), digest);
    assert_eq!(
        endpoint.configuration().execution_limits().effective(),
        &chigiri::ExecutionLimits::production()
    );
}
