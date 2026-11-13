//! Verify real HTTP terminal logs, hook correlation, and automatic-log secrecy.

use std::collections::BTreeSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use chigiri_mock::hooks::HookSlot;
use serde_json::{Value, json};

use super::hooks_http::with_hook;
use super::support::{Server, TEMPLATE, request_path};

pub(super) fn records(server: &Server) -> Vec<Value> {
    let bytes = fs::read(server.directory.0.join("stdout.log")).unwrap();
    assert!(bytes.ends_with(b"\n"));
    assert!(!bytes.contains(&27));
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).expect("stdout is exclusive to JSONL.");
            assert_eq!(record["format"], chigiri_mock::logging::FORMAT);
            record
        })
        .collect()
}

fn configuration() -> String {
    let mut config = TEMPLATE.replace("Alice", "RESULT_SECRET");
    for slot in HookSlot::ALL {
        let ending = if slot == HookSlot::RequestAfter {
            "error('RUNTIME_SECRET')"
        } else {
            "return nil"
        };
        config = with_hook(
            &config,
            slot,
            &format!(
                "local private = 'SOURCE_SECRET'\nctx.log('info', 'chosen\\n\\027]0;title\\007', {{kind='spoof', request_id=999}})\n{ending}"
            ),
        );
    }
    let mut document = config.parse::<toml_edit::DocumentMut>().unwrap();
    document["state"]["contract_json"] = toml_edit::value(
        json!({"fields":{"secret":{"kind":"string"}},"initial":{"secret":"STATE_SECRET"}})
            .to_string(),
    );
    document.to_string()
}

#[test]
fn hk_15_16_18_http_routes_correlate_all_slots_escape_and_omit_automatic_secrets() {
    let _case = crate::support::test_case();
    let earliest = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut server = Server::with_graphiql(&configuration(), false);
    let response = request_path(
        server.address,
        "POST",
        "/graphql?private=QUERY_SECRET",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer TOKEN_SECRET"),
            ("Cookie", "session=COOKIE_SECRET"),
        ],
        &json!({"query":"query DOCUMENT_SECRET { chosen:getUsers(limit:1) { name } }"}).to_string(),
    );
    assert_eq!(response.status, 500);
    let failure: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(failure["data"], Value::Null);
    assert_eq!(failure["errors"].as_array().unwrap().len(), 1);
    assert_eq!(failure["errors"][0]["message"], "Mock execution failed");
    assert_eq!(
        failure["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert!(!response.body.contains("RESULT_SECRET"));
    assert!(!response.body.contains("RUNTIME_SECRET"));
    let mut expected = vec![("POST", "/graphql", 500)];
    for (method, path, status) in [
        ("GET", "/graphiql", 200),
        ("HEAD", "/graphiql", 200),
        ("GET", "/graphiql/assets/app.js", 200),
        ("HEAD", "/graphiql/assets/app.js", 200),
        ("GET", "/missing", 404),
    ] {
        assert_eq!(
            request_path(server.address, method, path, &[], "").status,
            status
        );
        expected.push((method, path, status));
    }
    for (method, headers, body, status) in [
        (
            "OPTIONS",
            vec![
                ("Origin", "http://localhost:5173"),
                ("Access-Control-Request-Method", "POST"),
            ],
            String::new(),
            204,
        ),
        (
            "POST",
            vec![("Origin", "http://denied.invalid")],
            "{}".into(),
            403,
        ),
        (
            "POST",
            vec![("Content-Type", "application/json")],
            "{BODY_SECRET".into(),
            400,
        ),
        (
            "POST",
            vec![("Content-Type", "application/json")],
            "x".repeat(1024 * 1024 + 1),
            413,
        ),
    ] {
        assert_eq!(server.http(method, &headers, &body).status, status);
        expected.push((method, "/graphql", status));
    }
    server.stop("-TERM");
    let latest = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let logs = records(&server);
    let mut accesses: Vec<_> = logs
        .iter()
        .filter(|record| record["kind"] == "access")
        .collect();
    accesses.sort_by_key(|record| record["request_id"].as_u64().unwrap());
    assert_eq!(accesses.len(), expected.len());
    assert_eq!(
        accesses
            .iter()
            .map(|record| record["request_id"].as_u64().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        expected.len()
    );
    for (record, (method, path, status)) in accesses.iter().zip(expected) {
        assert_eq!(record["method"], method);
        assert_eq!(record["path"], path);
        assert_eq!(record["status"], status);
        assert_eq!(record["outcome"], "completed");
        assert!(record["duration_ms"].as_f64().unwrap() >= 0.0);
    }
    let id = &accesses[0]["request_id"];
    assert_eq!(
        failure["errors"][0]["extensions"]["requestId"],
        id.as_u64().unwrap().to_string()
    );
    let hooks: Vec<_> = logs
        .iter()
        .filter(|record| record["kind"] == "hook" && &record["request_id"] == id)
        .collect();
    for slot in HookSlot::ALL {
        assert!(
            hooks.iter().any(|record| {
                record["phase"] == serde_json::to_value(slot.phase()).unwrap()
                    && record["timing"] == serde_json::to_value(slot.timing()).unwrap()
                    && record["level"] == "info"
            }),
            "{}",
            slot.name()
        );
    }
    assert!(
        hooks
            .iter()
            .any(|record| record["coordinate"] == "User.name"
                && record["path"] == json!(["chosen", 0, "name"]))
    );
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let diagnostics: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    assert!(
        diagnostics
            .iter()
            .any(|record| record["diagnostic"]["phase"] == "runtime"
                && record["diagnostic"]["module"] == "mock.toml"
                && &record["request_id"] == id
                && record["message"] == "Mock hook failed")
    );
    assert!(hooks.iter().all(|record| record["diagnostic"].is_null()));
    for record in &logs {
        let timestamp = record["timestamp"].as_u64().unwrap() as u128;
        assert!((earliest..=latest).contains(&timestamp));
        assert_ne!(record["kind"], "log_loss");
        if record["level"] == "info" {
            assert_eq!(record["fields"], json!({"kind":"spoof", "request_id":999}));
            assert_eq!(record["message"], "chosen\n\u{1b}]0;title\u{7}");
        }
    }
    let output = fs::read_to_string(server.directory.0.join("stdout.log")).unwrap();
    for secret in [
        "QUERY_SECRET",
        "TOKEN_SECRET",
        "COOKIE_SECRET",
        "BODY_SECRET",
        "DOCUMENT_SECRET",
        "RESULT_SECRET",
        "STATE_SECRET",
        "SOURCE_SECRET",
        "RUNTIME_SECRET",
    ] {
        assert!(
            !output.contains(secret),
            "Automatic logs must not expose request secrets."
        );
        assert!(!stderr.contains(secret), "stderr exposed a request secret");
    }
    assert!(!output.contains("chigiri mock listening"));
    let notice = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    assert!(notice.contains("chigiri mock listening"));
    assert!(notice.contains("chigiri mock GraphiQL"));
}

#[test]
fn hk_16_access_logging_is_enabled_without_hooks() {
    let _case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    assert!(server.graphql("{ getUsers { id } }")["errors"].is_null());
    server.stop("-INT");
    let logs = records(&server);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["kind"], "access");
    assert_eq!(logs[0]["status"], 200);
    assert_eq!(logs[0]["outcome"], "completed");
}

#[test]
fn hk_17_18_real_slow_closed_stdio_keeps_http_and_reaps_writers_within_shared_deadline() {
    let _case = crate::support::test_case();
    let directory = crate::support::Directory::new();
    let config = with_hook(
        TEMPLATE,
        HookSlot::RequestBefore,
        "local message = 'x' for i = 1, 12 do message = message .. message end for i = 1, 64 do ctx.log('info', message) end return nil",
    );
    let path = directory.0.join("logging.toml");
    fs::write(&path, config).unwrap();
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../mock/tests/acceptance/logging_io.py"
        ))
        .arg(crate::support::BINARY)
        .arg(path)
        .arg(&directory.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}

pub(super) fn assert_access_statuses(server: &Server, expected: &[u16]) {
    let logs = records(server);
    let mut accesses: Vec<_> = logs
        .iter()
        .filter(|record| record["kind"] == "access")
        .collect();
    accesses.sort_by_key(|record| record["request_id"].as_u64().unwrap());
    assert_eq!(accesses.len(), expected.len());
    for (record, status) in accesses.iter().zip(expected) {
        assert_eq!(record["status"], *status);
        assert_eq!(record["outcome"], "completed");
    }
}
