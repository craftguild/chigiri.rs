//! Run enabled consumers, references and detached bundles through the real product.

use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{assertions::Profile, http, websocket};
use crate::support::{Directory, Server};

mod project;

const QUERY: &str = "subscription { changed(required:\"wire\") { id value } }";

#[test]
fn ev_05_06_08_11_12_enabled_consumers_work_without_clients_and_survive_detached_bundle_restart() {
    let _case = crate::support::test_case();
    let directory = project::checked();
    reference(&directory, "--config-dir", "mock", "project");
    project::success(
        &directory,
        &[
            "export",
            "--config-dir",
            "mock",
            "--output",
            "portable.toml",
        ],
    );
    let bundle = fs::read_to_string(directory.0.join("portable.toml")).unwrap();
    assert!(bundle.contains("chigiri-mock/v11") && bundle.contains("chigiri-mock-capabilities/v4"));
    let mut server = Server::project(Directory(directory.0.clone()));
    assert_background(&server);
    wire(&server);
    transaction(&server);
    server.stop("-TERM");
    assert_logs(&server);

    // The same exported bytes run without project, public SDL, helper files or
    // PATH. No readiness/reference check may reactivate old queued callbacks.
    fs::remove_dir_all(directory.0.join("mock")).unwrap();
    fs::remove_file(directory.0.join("schema.graphql")).unwrap();
    reference(&directory, "--config", "portable.toml", "bundle");
    for _ in 0..2 {
        let mut detached = Server::offline(&bundle);
        assert_background(&detached);
        assert_eq!(
            detached.graphql("{items{id value}}")["data"]["items"],
            json!([{"id":"clock","value":7}])
        );
        wire(&detached);
        detached.stop("-TERM");
        assert_logs(&detached);
    }
}

#[test]
fn ev_05_partial_startup_fails_before_listening_and_a_fresh_start_recovers() {
    let _case = crate::support::test_case();
    let directory = project::checked();
    let path = directory.0.join("mock/mock.luau");
    let original = fs::read_to_string(&path).unwrap();
    let failing = original.replace(
        "disabled = {field='changed',enabled=false},",
        "disabled = {field='changed',enabled=false},\nzz_fail = {field='changed',enabled=true},",
    );
    assert_ne!(failing, original);
    fs::write(&path, failing).unwrap();
    project::success(&directory, &["codegen", "--config-dir", "mock"]);
    // The generated intentional error remains a valid static contract. Source
    // establishment happens only at serve, after earlier registrations prepare.
    project::success(&directory, &["check", "--config-dir", "mock"]);
    let failed = directory.command(&["serve", "--config-dir", "mock"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&failed.stderr).contains("listening http://"));
    for line in String::from_utf8(failed.stdout).unwrap().lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        assert_ne!(record["outcome"], "committed");
        assert_ne!(record["outcome"], "rolled_back");
    }
    fs::write(path, original).unwrap();
    project::success(&directory, &["codegen", "--config-dir", "mock"]);
    let mut server = Server::project(directory);
    assert_background(&server);
    server.stop("-TERM");
}

fn reference(directory: &Directory, flag: &str, path: &str, input: &str) {
    for name in ["pubsub", "generator"] {
        let bytes = project::success(
            directory,
            &["reference", "runtime", name, flag, path, "--format", "json"],
        );
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["format"], "chigiri-mock-reference-topic/v9");
        assert_eq!(document["input_kind"], input);
        assert_eq!(document["readiness_checked"], false);
        assert!(String::from_utf8(bytes).unwrap().contains(".Item("));
    }
    let bytes = project::success(
        directory,
        &[
            "reference",
            "consumers",
            "disabled",
            flag,
            path,
            "--format",
            "json",
        ],
    );
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["input_kind"], input);
    assert_eq!(document["content"]["items"][0]["enabled"], false);
    assert_eq!(
        document["content"]["items"][0]["source"]["coordinate"],
        "Signals.changed"
    );
}

fn assert_background(server: &Server) {
    until(|| {
        let audits = audits(server);
        audits
            .iter()
            .any(|row| row["id"] == "source" && row["value"] == 7 && row["depth"] == 0)
            && audits.iter().any(|row| {
                row["id"] == "out" && row["value"] == 7 && row["depth"] == 1 && row["counter"] == 0
            })
    });
}

fn wire(server: &Server) {
    let expected = json!({"id":"clock","value":7});
    let mut sse = http::hold(server.address, Profile::ServerSentEvents, QUERY);
    let mut multipart = http::hold(server.address, Profile::Multipart, QUERY);
    let mut socket = websocket::hold(server.address, QUERY);
    assert_eq!(sse.next_changed(), expected);
    assert_eq!(multipart.next_changed(), expected);
    assert_eq!(socket.next_changed(), expected);
    socket.complete();
}

fn transaction(server: &Server) {
    let response = server.http(
        "POST",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer publisher-secret"),
        ],
        &json!({"query":"mutation{publish(id:\"bad\",value:13,topic:\"input\")}"}).to_string(),
    );
    assert_eq!(response.status, 200);
    until(|| {
        log_records(server).iter().any(|record| {
            record["kind"] == "consumer_terminal"
                && record["consumer"] == "process"
                && record["outcome"] == "rolled_back"
        })
    });
    let rows = audits(server);
    assert!(
        !rows
            .iter()
            .any(|row| row["id"] == "candidate-bad" || row["id"] == "child-bad")
    );
    let items = server.graphql("{items{id value}}");
    assert!(
        items["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "bad" && row["value"] == 13)
    );
    assert_eq!(
        server.graphql("mutation{publish(id:\"good\",value:14,topic:\"input\")}")["data"]["publish"],
        1
    );
    until(|| {
        audits(server)
            .iter()
            .any(|row| row["id"] == "child-good" && row["depth"] == 1 && row["value"] == 14)
    });
    assert!(
        audits(server)
            .iter()
            .any(|row| row["id"] == "candidate-good" && row["depth"] == 0)
    );
}

fn audits(server: &Server) -> Vec<Value> {
    let result = server.graphql("{audits{id value depth counter}}");
    assert!(result.get("errors").is_none(), "{result}");
    result["data"]["audits"].as_array().unwrap().clone()
}

fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < deadline, "consumer observation watchdog");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn log_records(server: &Server) -> Vec<Value> {
    let text = fs::read_to_string(server.directory.0.join("stdout.log")).unwrap();
    assert!(
        text.len() <= 8 * 1024 * 1024,
        "bounded fixture log observation"
    );
    // An isolated writer can be in the middle of the final line while polled.
    // Every complete line is still required to be valid JSON.
    text.split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .map(|line| serde_json::from_str(line).expect("complete log record"))
        .collect()
}

fn assert_logs(server: &Server) {
    assert!(
        fs::read_to_string(server.directory.0.join("stdout.log"))
            .unwrap()
            .ends_with('\n')
    );
    let records = log_records(server);
    assert!(
        records
            .iter()
            .all(|record| record["format"] == "chigiri-mock-log/v5")
    );
    assert!(records.iter().any(|record| record["kind"]=="consumer_terminal" && record["outcome"]=="committed"));
    assert!(
        !records
            .iter()
            .any(|record| record["consumer"] == "disabled")
    );
    let automatic: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "consumer_terminal")
        .collect();
    let text = serde_json::to_string(&automatic).unwrap();
    for secret in [
        "publisher-secret",
        "Authorization",
        "publisher",
        "payload",
        "headers",
    ] {
        assert!(
            !text.contains(secret),
            "automatic consumer logs retained {secret}"
        );
    }
}
