//! Verify project and bundle references report the same state types without executing hooks.

use std::fs;
use std::process::Command;

use serde_json::{Value, json};

use super::harness::{BLOG, CONFIG, Directory};

#[test]
fn hk_03_06_19_cli_uses_configured_state_and_registration_without_readiness_or_initial_values() {
    let directory = Directory::new();
    fs::write(directory.0.join("blog.graphql"), BLOG).unwrap();
    assert!(
        directory
            .run(&["init", "--schema", "blog.graphql", "--config-dir", "mock"])
            .status
            .success()
    );
    let project = directory.0.join("mock");
    let path = project.join("mock.luau");
    let config = r#"local config: ChigiriMockProjectConfig = {
        format="chigiri-mock-project/v10",
        schema={sources={"../blog.graphql"}},
        storage={sources={}},
        clock={mode="system"},
        server={listen="127.0.0.1:0",allowed_origins={}},
        hooks={request={before=true,after=false},operation={before=true,after=false},field={before=true,after=false}},
        state={fields={auth={kind="boolean"},secret={kind="string"}},initial={auth=false,secret="INITIAL_PRIVATE"}},
    }
    return config"#;
    fs::write(&path, config).unwrap();
    fs::remove_file(directory.0.join("blog.graphql")).unwrap();
    let codegen = directory.run(&["codegen", "--config-dir", "mock"]);
    assert!(
        codegen.status.success(),
        "{}",
        String::from_utf8_lossy(&codegen.stderr)
    );
    for name in ["hooks", "resolvers", "seeds"] {
        fs::rename(project.join(name), project.join(format!("kept-{name}"))).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(project.join(name))
                .status()
                .unwrap()
                .success()
        );
    }
    let output = directory.run(&["reference", "--config-dir", "mock", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let project_doc: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(project_doc["format"], "chigiri-mock-reference/v13");
    assert_eq!(project_doc["api_version"], "chigiri-mock-api/v10");
    assert_eq!(project_doc["config_executed"], true);
    assert_eq!(project_doc["state_configured"], true);
    assert_eq!(project_doc["readiness_checked"], false);
    for (index, hook) in project_doc["hooks"].as_array().unwrap().iter().enumerate() {
        assert_eq!(hook["enabled"], index % 2 == 0);
    }
    let shape = project_doc["api_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "ChigiriState")
        .unwrap();
    assert_eq!(
        shape["shape"]["fields"],
        json!([
            {"name":"auth","type_id":"boolean","required":true,"nullable":false},
            {"name":"secret","type_id":"json_string","required":true,"nullable":false}
        ])
    );
    let mut bundle = CONFIG.parse::<toml_edit::DocumentMut>().unwrap();
    bundle["state"]["contract_json"]=toml_edit::value(json!({"fields":{"auth":{"kind":"boolean"},"secret":{"kind":"string"}},"initial":{"auth":false,"secret":"DIFFERENT_PRIVATE"}}).to_string());
    for phase in ["request", "operation", "field"] {
        bundle["hooks"][phase]["before"] = toml_edit::value(true);
        let mut module = toml_edit::Table::new();
        module["luau"] = toml_edit::value("INVALID_PRIVATE_HOOK require('outside')");
        bundle["hooks"]["modules"][format!("{phase}.before")] = toml_edit::Item::Table(module);
    }
    bundle["seed"]["records_json"] = toml_edit::value("PRIVATE_BAD_SEED");
    fs::write(directory.0.join("mock.toml"), bundle.to_string()).unwrap();
    let output = directory.run(&["reference", "--config", "mock.toml", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle_doc: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bundle_doc["config_executed"], false);
    assert_eq!(bundle_doc["state_configured"], true);
    assert_eq!(bundle_doc["api_types"], project_doc["api_types"]);
    assert_eq!(bundle_doc["hooks"], project_doc["hooks"]);
    for document in [&project_doc, &bundle_doc] {
        let json = serde_json::to_string(document).unwrap();
        for secret in [
            "INITIAL_PRIVATE",
            "DIFFERENT_PRIVATE",
            "INVALID_PRIVATE_HOOK",
            "PRIVATE_BAD_SEED",
        ] {
            assert!(!json.contains(secret));
        }
    }
    for (flag, input) in [("--config-dir", "mock"), ("--config", "mock.toml")] {
        let output = directory.run(&["reference", "all", flag, input]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("state_configured: true"));
        for secret in [
            "INITIAL_PRIVATE",
            "DIFFERENT_PRIVATE",
            "INVALID_PRIVATE_HOOK",
            "PRIVATE_BAD_SEED",
        ] {
            assert!(!text.contains(secret));
        }
        assert_eq!(
            directory.run(&["check", flag, input]).status.code(),
            Some(1)
        );
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), config);
}
