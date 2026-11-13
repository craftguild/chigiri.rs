//! Compare directory and offline-bundle behavior for both domains, including initial seed export.

use std::fs;

use chigiri_mock::project::{Layout, read_project_schema};
use serde_json::{Value, json};

use crate::support::{Directory, Server, TEMPLATE, seed, with_seed};

pub(super) fn project(source: &str, config: &str) -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), source).unwrap();
    let result = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let root = directory.0.join("mock");
    let schema = read_project_schema(&root).unwrap();
    let layout = Layout::new(schema.ir()).unwrap();
    let config_doc = config.parse::<toml_edit::DocumentMut>().unwrap();
    for entry in layout.resolvers {
        fs::write(
            root.join(entry.path.as_str()),
            config_doc["resolvers"][&entry.coordinate]["luau"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    }
    for (model, rows) in seed(config).as_object().unwrap() {
        let mut jsonl = String::new();
        for row in rows.as_array().unwrap() {
            jsonl.push_str(&row.to_string());
            jsonl.push('\n');
        }
        fs::write(root.join(format!("seeds/{model}.jsonl")), jsonl).unwrap();
    }
    let path = root.join("mock.luau");
    let config = fs::read_to_string(&path).unwrap();
    assert!(config.contains("../schema.graphql"));
    fs::write(path, config.replace("127.0.0.1:4000", "127.0.0.1:0")).unwrap();
    directory
}

#[test]
fn mp_08_16_25_27_two_domain_directory_and_offline_bundle_http_are_equivalent() {
    let _test_case = crate::support::test_case();
    let projects = crate::runtime_schema::projects();
    let projects = with_seed(
        &projects,
        &json!({
            "Member":[{"id":"Member-9","name":"Ada","status":"OPEN","code":"AA"}], "Project":[], "Task":[]
        }),
    );
    for (source, config, query, mutation) in [
        (
            include_str!("../../fixtures/blog.graphql"),
            TEMPLATE,
            "{getUsers{id name posts{id title}}}",
            "mutation{createUser(input:{name:\"Transient\"}){id name}}",
        ),
        (
            include_str!("../../fixtures/projects.graphql"),
            projects.as_str(),
            "{members{id name label(prefix:\"member:\")}}",
            "mutation{addMember(input:{name:\"Transient\",code:\"BB\"}){id name status}}",
        ),
    ] {
        let directory = project(source, config);
        let check = directory.command(&["check", "--config-dir", "mock"]);
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
        assert!(
            directory
                .command(&["export", "--config-dir", "mock", "--output", "before.toml"])
                .status
                .success()
        );
        let initial_bundle = fs::read_to_string(directory.0.join("before.toml")).unwrap();
        let mut server = Server::project(directory);
        let initial = server.graphql(query);
        assert!(initial.get("errors").is_none(), "{initial}");
        let mutation_result = server.graphql(mutation);
        assert!(mutation_result.get("errors").is_none(), "{mutation_result}");
        let after = server.graphql(query);
        assert_ne!(initial, after);
        assert!(
            server
                .directory
                .command(&["export", "--config-dir", "mock", "--output", "after.toml"])
                .status
                .success()
        );
        let bundle = fs::read_to_string(server.directory.0.join("after.toml")).unwrap();
        assert_eq!(bundle, initial_bundle);
        server.stop("-TERM");
        fs::rename(
            server.directory.0.join("mock"),
            server.directory.0.join("unavailable-project"),
        )
        .unwrap();
        fs::remove_file(server.directory.0.join("schema.graphql")).unwrap();
        assert!(!server.directory.0.join("mock").exists());
        let mut offline = Server::offline(&bundle);
        assert_eq!(offline.graphql(query), initial);
        let offline_mutation = offline.graphql(mutation);
        let offline_row = offline_mutation["data"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        let live_row = mutation_result["data"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_eq!(offline_row["name"], "Transient");
        assert!(offline_row["id"].is_string());
        assert!(live_row["id"].is_string());
        let offline_after = offline.graphql(query);
        let offline_rows = offline_after["data"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_array()
            .unwrap();
        let live_rows = after["data"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(offline_rows.len(), live_rows.len());
        for (offline_row, live_row) in offline_rows.iter().zip(live_rows) {
            assert_eq!(offline_row["name"], live_row["name"]);
        }
        offline.stop("-TERM");
    }
}

#[test]
fn mp_08_16_project_check_never_resolves_and_stderr_diagnostics_use_logical_module_path() {
    let _test_case = crate::support::test_case();
    let config = crate::support::edited("Query.getUsers", "error('private-value')");
    let directory = project(include_str!("../../fixtures/blog.graphql"), &config);
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    let mut server = Server::project(directory);
    let http = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"{getUsers{id}}"}).to_string(),
    );
    assert_eq!(http.status, 500);
    let response: Value = serde_json::from_str(&http.body).unwrap();
    assert_eq!(response["data"], Value::Null);
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    let message = response["errors"][0]["message"].as_str().unwrap();
    assert_eq!(message, "Mock execution failed");
    assert!(!message.contains("private-value"));
    assert!(!message.contains(server.directory.0.to_str().unwrap()));
    server.stop("-TERM");
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let diagnostics: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == "resolver_diagnostic")
        .collect();
    assert_eq!(diagnostics.len(), 1, "{stderr}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["module"], "resolvers/query/getUsers.luau");
    assert_eq!(diagnostic["coordinate"], "Query.getUsers");
    assert_eq!(diagnostic["phase"], "runtime");
    assert!(diagnostic["line"].as_u64().unwrap() > 0);
    assert_eq!(
        response["errors"][0]["extensions"]["requestId"],
        diagnostic["request_id"].as_u64().unwrap().to_string()
    );
    assert!(!stderr.contains("private-value"));
    assert!(!stderr.contains(server.directory.0.to_str().unwrap()));
}

#[test]
fn mp_01_08_28_cli_rejects_legacy_mixed_duplicate_and_unknown_options() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    for args in [
        vec!["init", "--schema", "a", "--out", "x"],
        vec![
            "init",
            "--schema",
            "a",
            "--config-dir",
            "x",
            "--config-dir",
            "y",
        ],
        vec!["check"],
        vec!["update"],
        vec!["update", "--config", "a"],
        vec!["update", "--config-dir", "a", "--config-dir", "b"],
        vec!["update", "--config-dir", "a", "--schema", "b"],
        vec!["update", "--config-dir", "a", "--force"],
        vec!["check", "--config", "a", "--config-dir", "b"],
        vec!["check", "--config", "a", "--config", "a"],
        vec!["serve", "--config-dir", "a", "--schema", "b"],
        vec!["serve", "--config-dir", "a", "--config-dir", "b"],
        vec!["export", "--config", "a", "--output", "b"],
        vec!["export", "--config-dir", "a"],
        vec![
            "export",
            "--config-dir",
            "a",
            "--output",
            "b",
            "--output",
            "c",
        ],
        vec!["export", "--config-dir", "a", "--output", "b", "--force"],
        vec!["reference", "--config-dir", "a", "--config", "b"],
        vec!["reference", "--config-dir", "a", "--schema", "b"],
        vec!["reference", "--config-dir", "a", "--config-dir", "a"],
    ] {
        let output = directory.command(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
    }
}
