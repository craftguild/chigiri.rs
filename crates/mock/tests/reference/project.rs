//! Verify project reference uses adopted SDL and generation metadata without checking readiness.

use std::fs;
use std::process::Command;

use serde_json::Value;

use super::harness::{BLOG, Directory};

#[test]
fn as_03_04_reference_uses_adopted_schema_and_ignores_unreadable_resolver_seed() {
    let directory = Directory::new();
    fs::write(directory.0.join("blog.graphql"), BLOG).unwrap();
    let result = directory.run(&["init", "--schema", "blog.graphql", "--config-dir", "mock"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let schema = directory.run(&["reference", "--schema", "blog.graphql", "--format", "json"]);
    assert!(schema.status.success());
    let project = directory.0.join("mock");
    fs::write(
        project.join("mock.luau"),
        r#"--!strict
local private = "PRIVATE_CONFIG_SOURCE"
local prefix = "../"
local config: ChigiriMockProjectConfig = {
    hooks = {request = {before = false, after = false}, operation = {before = false, after = false}, field = {before = false, after = false}},
    state = {fields = {}, initial = {}},
    format = "chigiri-mock-project/v10",
    schema = {sources = {prefix .. "blog.graphql"}},
    storage = {sources = {"storage.graphql"}},
    clock = {mode = "system"},
    server = {listen = "127.0.0.1:0", allowed_origins = {}},
}
return config
"#,
    )
    .unwrap();
    fs::remove_file(directory.0.join("blog.graphql")).unwrap();
    for name in ["resolvers", "seeds"] {
        fs::rename(project.join(name), project.join(format!("saved-{name}"))).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(project.join(name))
                .status()
                .unwrap()
                .success()
        );
    }
    let args = ["reference", "--config-dir", "mock", "--format", "json"];
    let result = directory.run(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(doc["format"], "chigiri-mock-reference/v13");
    assert_eq!(doc["input_kind"], "project");
    assert_eq!(doc["config_executed"], true);
    assert_eq!(doc["readiness_checked"], false);
    assert_eq!(doc["mock_format"], "chigiri-mock-project/v10");
    let schema_doc: Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(schema_doc["input_kind"], "schema");
    assert_eq!(
        doc["schema"]["models"].as_array().unwrap().len(),
        schema_doc["schema"]["models"].as_array().unwrap().len()
    );
    let text = directory.run(&["reference", "all", "--config-dir", "mock"]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("config_executed: true"));
    assert!(text.contains("readiness_checked: false"));
    for body in [String::from_utf8(result.stdout).unwrap(), text] {
        for secret in [
            "PRIVATE_CONFIG_SOURCE",
            "blog.graphql",
            directory.0.to_str().unwrap(),
        ] {
            assert!(!body.contains(secret), "{secret}");
        }
    }
    assert_eq!(
        directory
            .run(&["check", "--config-dir", "mock"])
            .status
            .code(),
        Some(1)
    );
    fs::write(
        project.join("mock.luau"),
        "return {} -- PRIVATE_INVALID_CONFIG",
    )
    .unwrap();
    let rejected = directory.run(&args);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&rejected.stderr).contains("PRIVATE_INVALID_CONFIG"));
}

#[test]
fn r52_03_named_directive_and_hook_text_retain_project_authority() {
    let directory = Directory::new();
    fs::write(
        directory.0.join("schema.graphql"),
        concat!(
            "\"Adopted policy description.\" directive @policy on FIELD_DEFINITION ",
            "type User { id: ID! } type Query { users: [User!]! @policy }",
        ),
    )
    .unwrap();
    let initialized =
        directory.run(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(initialized.status.success());
    let config_path = directory.0.join("mock/mock.luau");
    let disabled = fs::read_to_string(&config_path).unwrap();
    let enabled = disabled.replace("request = { before = false", "request = { before = true");
    assert_ne!(enabled, disabled);
    fs::write(config_path, enabled).unwrap();
    assert!(
        directory
            .run(&["codegen", "--config-dir", "mock"])
            .status
            .success()
    );

    for (topic, name) in [("directives", "policy"), ("hooks", "request.before")] {
        let result = directory.run(&["reference", topic, name, "--config-dir", "mock"]);
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(
            text.contains(
                "Project input uses adopted public SDL plus current project-local storage"
            )
        );
        assert!(text.contains(&format!(
            "chigiri mock reference {topic} {name} --config-dir PROJECT"
        )));
        assert!(!text.contains("--schema SDL"));
        assert!(!text.contains(directory.0.to_str().unwrap()));
        if topic == "directives" {
            assert!(text.contains("Adopted policy description."));
        } else {
            assert!(text.contains("enabled=Some(true)"));
        }

        let result = directory.run(&[
            "reference",
            topic,
            name,
            "--config-dir",
            "mock",
            "--format",
            "json",
        ]);
        assert!(result.status.success());
        let json: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(json["input_kind"], "project");
        assert_eq!(json["readiness_checked"], false);
    }
}
