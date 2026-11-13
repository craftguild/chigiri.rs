//! Verify check-v3 source positions and input-specific reference guidance.

use serde_json::Value;
use std::fs;

use crate::support::{Directory, TEMPLATE, edited};

#[test]
fn af_09_10_bundle_json_separates_locations_and_uses_bundle_guidance() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    let invalid = TEMPLATE.replacen(
        "allowed_origins = [\"http://localhost:5173\"]",
        "allowed_origins = ]",
        1,
    );
    fs::write(directory.0.join("invalid.toml"), invalid).unwrap();

    let text = directory.command(&["check", "--config", "invalid.toml"]);
    assert_eq!(text.status.code(), Some(1));
    let text = String::from_utf8(text.stderr).unwrap();
    let location = text.split_whitespace().next().unwrap();
    let mut parts = location.rsplitn(3, ':');
    let column: usize = parts.next().unwrap().parse().unwrap();
    let line: usize = parts.next().unwrap().parse().unwrap();
    assert!(line > 1);

    let json = directory.command(&["check", "--config", "invalid.toml", "--format", "json"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let report: Value = serde_json::from_slice(&json.stdout).unwrap();
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["file"], "invalid.toml");
    assert_eq!(diagnostic["coordinate"], Value::Null);
    assert_eq!(diagnostic["member"], Value::Null);
    assert_eq!(diagnostic["line"], line);
    assert_eq!(diagnostic["column"], column);
    assert!(diagnostic["embedded"].is_null());

    let invalid_luau = edited("Query.getUsers", "if then");
    fs::write(directory.0.join("invalid-luau.toml"), invalid_luau).unwrap();
    let json = directory.command(&["check", "--config", "invalid-luau.toml", "--format", "json"]);
    assert_eq!(json.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&json.stdout).unwrap();
    let diagnostic = &report["diagnostics"][0];
    assert!(diagnostic["line"].as_u64().unwrap() > 1);
    assert!(diagnostic["embedded"].is_null());
    assert!(
        diagnostic["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["topic"] == "resolvers")
    );
    let text = directory.command(&["check", "--config", "invalid-luau.toml"]);
    assert_eq!(text.status.code(), Some(1));
    let text = String::from_utf8(text.stderr).unwrap();
    assert!(text.contains("--config <BUNDLE>"));
    assert!(text.contains("replace <BUNDLE>"));
    assert!(!text.contains(directory.0.to_str().unwrap()));

    let missing = directory.command(&["check", "--config", "missing.toml", "--format", "json"]);
    assert_eq!(missing.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&missing.stdout).unwrap();
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["file"], "missing.toml");
    for field in ["coordinate", "member", "line", "column"] {
        assert_eq!(diagnostic[field], Value::Null, "{field}");
    }
}
