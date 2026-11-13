//! Exercise the public-schema freshness protocol through the real CLI.

use std::fs;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::support::{BINARY, Directory};

fn schema(argument: &str) -> String {
    format!(
        "type Item {{id:ID! title:String!}} type Query {{items({argument}:Int! = 20):[Item!]!}}"
    )
}

fn project() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), schema("first")).unwrap();
    let result = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(result.status.success());
    directory
}

fn management_bytes(directory: &Directory) -> Vec<Vec<u8>> {
    [
        "mock/generated/types.d.luau",
        "mock/generated/schema.snapshot.json",
        "mock/generated/update-state.json",
    ]
    .into_iter()
    .map(|path| fs::read(directory.0.join(path)).unwrap())
    .collect()
}

#[test]
fn as_07_cli_public_comparison_has_exact_statuses_json_usage_and_pipe_behavior() {
    let _test_case = crate::support::test_case();
    let directory = project();
    let in_sync = directory.command(&["update", "--config-dir", "mock", "--check"]);
    assert!(in_sync.status.success());
    assert!(in_sync.stderr.is_empty());
    assert_eq!(
        String::from_utf8(in_sync.stdout).unwrap(),
        "chigiri mock update --check: in_sync; public SDL matches the adopted contract; readiness_checked = false\n"
    );
    let in_sync_json = directory.command(&[
        "update",
        "--config-dir",
        "mock",
        "--check",
        "--format",
        "json",
    ]);
    assert!(in_sync_json.status.success());
    assert!(in_sync_json.stderr.is_empty());
    let report: Value = serde_json::from_slice(&in_sync_json.stdout).unwrap();
    assert_eq!(report.as_object().unwrap().len(), 6);
    assert_eq!(report["format"], "chigiri-mock-schema-sync/v1");
    assert_eq!(report["status"], "in_sync");
    assert_eq!(report["readiness_checked"], false);
    assert_eq!(report["adopted_digest"], report["input_digest"]);
    assert_eq!(report["diagnostics"], serde_json::json!([]));

    let (reader, writer) = rustix::pipe::pipe().unwrap();
    drop(reader);
    let closed = Command::new(BINARY)
        .args(["mock", "update", "--config-dir", "mock", "--check"])
        .current_dir(&directory.0)
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(closed.status.success());
    assert!(closed.stderr.is_empty());

    fs::write(
        directory.0.join("mock/resolvers/query/items.luau"),
        "invalid pending resolver",
    )
    .unwrap();
    let config_path = directory.0.join("mock/mock.luau");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "storage = { sources = { \"storage.graphql\" } }",
        "storage = { sources = { \"missing.graphql\" } }",
    );
    fs::write(&config_path, config).unwrap();
    assert!(
        directory
            .command(&["update", "--config-dir", "mock", "--check"])
            .status
            .success()
    );

    fs::write(directory.0.join("schema.graphql"), schema("changed")).unwrap();
    let changed_text = directory.command(&["update", "--config-dir", "mock", "--check"]);
    assert_eq!(changed_text.status.code(), Some(1));
    assert!(changed_text.stderr.is_empty());
    assert_eq!(
        String::from_utf8(changed_text.stdout).unwrap(),
        "chigiri mock update --check: update_required; run `chigiri mock update` to adopt the current public SDL; readiness_checked = false\n"
    );
    let changed = directory.command(&[
        "update",
        "--config-dir",
        "mock",
        "--check",
        "--format",
        "json",
    ]);
    assert_eq!(changed.status.code(), Some(1));
    assert!(changed.stderr.is_empty());
    let report: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(report.as_object().unwrap().len(), 6);
    assert_eq!(report["format"], "chigiri-mock-schema-sync/v1");
    assert_eq!(report["status"], "update_required");
    assert_eq!(report["readiness_checked"], false);
    assert!(report["adopted_digest"].is_string());
    assert!(report["input_digest"].is_string());
    assert_eq!(report["diagnostics"], serde_json::json!([]));

    fs::write(directory.0.join("schema.graphql"), "invalid SDL").unwrap();
    let unavailable_text = directory.command(&["update", "--config-dir", "mock", "--check"]);
    assert_eq!(unavailable_text.status.code(), Some(2));
    assert!(unavailable_text.stderr.is_empty());
    assert_eq!(
        String::from_utf8(unavailable_text.stdout).unwrap(),
        "chigiri mock update --check: unavailable; schema: invalid SDL; public freshness is unknown; readiness_checked = false\n"
    );
    let unavailable = directory.command(&[
        "update",
        "--config-dir",
        "mock",
        "--check",
        "--format",
        "json",
    ]);
    assert_eq!(unavailable.status.code(), Some(2));
    assert!(unavailable.stderr.is_empty());
    let report: Value = serde_json::from_slice(&unavailable.stdout).unwrap();
    assert_eq!(report.as_object().unwrap().len(), 6);
    assert_eq!(report["format"], "chigiri-mock-schema-sync/v1");
    assert_eq!(report["status"], "unavailable");
    assert_eq!(report["readiness_checked"], false);
    assert!(report["adopted_digest"].is_string());
    assert!(report["input_digest"].is_null());
    assert_eq!(
        report["diagnostics"],
        serde_json::json!([{"phase": "schema", "message": "invalid SDL"}])
    );

    let usage = directory.command(&["update", "--config-dir", "mock", "--format", "json"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
}

#[test]
fn r52_01_public_comparison_does_not_require_a_private_relation_key() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    let original = concat!(
        "type User { id: ID! } ",
        "type Post { id: ID! } ",
        "type Query { posts: [Post!]! }",
    );
    fs::write(directory.0.join("schema.graphql"), original).unwrap();
    let init = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(init.status.success());
    fs::write(
        directory.0.join("mock/storage.graphql"),
        "extend type User { privateCode: ID! }\n",
    )
    .unwrap();
    assert!(
        directory
            .command(&["codegen", "--config-dir", "mock"])
            .status
            .success()
    );

    let related = concat!(
        "type User { id: ID! } ",
        "type Post { id: ID! author: User! ",
        "@relation(fields: [\"authorCode\"], references: [\"privateCode\"]) } ",
        "type Query { posts: [Post!]! }",
    );
    fs::write(directory.0.join("schema.graphql"), related).unwrap();
    let before = management_bytes(&directory);
    let pending = directory.command(&[
        "update",
        "--config-dir",
        "mock",
        "--check",
        "--format",
        "json",
    ]);
    assert_eq!(pending.status.code(), Some(1));
    let pending: Value = serde_json::from_slice(&pending.stdout).unwrap();
    assert_eq!(pending["status"], "update_required");
    assert!(pending["input_digest"].is_string());
    assert_ne!(pending["adopted_digest"], pending["input_digest"]);
    assert_eq!(management_bytes(&directory), before);

    assert!(
        directory
            .command(&["update", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    let adopted = management_bytes(&directory);
    let in_sync = directory.command(&["update", "--config-dir", "mock", "--check"]);
    assert!(in_sync.status.success());
    assert_eq!(
        String::from_utf8(in_sync.stdout).unwrap(),
        "chigiri mock update --check: in_sync; public SDL matches the adopted contract; readiness_checked = false\n"
    );
    let report = directory.command(&[
        "update",
        "--config-dir",
        "mock",
        "--check",
        "--format",
        "json",
    ]);
    assert!(report.status.success());
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["status"], "in_sync");
    assert_eq!(report["adopted_digest"], report["input_digest"]);
    assert_eq!(management_bytes(&directory), adopted);
}
