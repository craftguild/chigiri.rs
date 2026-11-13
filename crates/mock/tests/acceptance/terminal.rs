//! Verify HTTP status and independent request completion in the real binary.

use std::fs;

use chigiri_mock::hooks::HookSlot;
use serde_json::{Value, json};

use super::hooks_http::with_hook;
use super::support::{Server, TEMPLATE};

#[test]
fn ra_terminal_separates_success_reject_after_failure_and_preexecution() {
    let _case = crate::support::test_case();
    let mutation = "mutation{createUser(input:{name:\"terminal-secret\"}){id}}";
    for (config, query, status, transaction, cause, unreached) in [
        (
            TEMPLATE.to_owned(),
            mutation,
            200,
            "committed",
            "success",
            vec![],
        ),
        (
            with_hook(
                &with_hook(
                    &with_hook(TEMPLATE, HookSlot::FieldAfter, "return nil"),
                    HookSlot::OperationAfter,
                    "return nil",
                ),
                HookSlot::RequestAfter,
                "return nil",
            ),
            mutation,
            200,
            "committed",
            "success",
            vec![],
        ),
        (
            with_hook(
                TEMPLATE,
                HookSlot::RequestAfter,
                "error('after-secret') return nil",
            ),
            mutation,
            500,
            "rolled_back",
            "execution_failed",
            vec![],
        ),
        (
            with_hook(
                &with_hook(TEMPLATE, HookSlot::RequestAfter, "return nil"),
                HookSlot::RequestBefore,
                "ctx.reject({code=200}) return nil",
            ),
            mutation,
            200,
            "rolled_back",
            "rejected",
            vec!["request.after"],
        ),
        (
            TEMPLATE.to_owned(),
            "{noSuchField}",
            422,
            "not_started",
            "not_started",
            vec![],
        ),
        (
            with_hook(
                &with_hook(TEMPLATE, HookSlot::RequestAfter, "return nil"),
                HookSlot::OperationAfter,
                "error('after-secret') return nil",
            ),
            mutation,
            500,
            "rolled_back",
            "execution_failed",
            vec!["request.after"],
        ),
        (
            with_hook(
                &with_hook(TEMPLATE, HookSlot::FieldAfter, "return nil"),
                HookSlot::FieldBefore,
                "ctx.reject({code=200}) return nil",
            ),
            mutation,
            200,
            "rolled_back",
            "rejected",
            vec!["field.after"],
        ),
    ] {
        let mut server = Server::start(&config);
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &json!({"query":query}).to_string(),
        );
        assert_eq!(response.status, status, "{}", response.body);
        let body: Value = serde_json::from_str(&response.body).unwrap();
        if transaction == "committed" {
            assert!(body.get("errors").is_none());
            assert_eq!(body["data"]["createUser"]["id"], "User-3");
        } else {
            assert!(!body["errors"].as_array().unwrap().is_empty());
            if transaction == "rolled_back" {
                assert_eq!(body["data"], Value::Null);
            }
        }
        server.stop("-TERM");
        let stdout = super::logging::records(&server);
        let access = stdout
            .iter()
            .find(|record| record["kind"] == "access")
            .unwrap();
        let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
        let events: Vec<Value> = stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|record| record["kind"] == "request_terminal")
            .collect();
        assert_eq!(events.len(), 1, "{stderr}");
        let event = &events[0];
        assert_eq!(event["request_id"], access["request_id"]);
        assert_eq!(event["status"], status);
        assert_eq!(event["transaction"], transaction);
        assert_eq!(event["cause"], cause);
        assert_eq!(event["phase"], "response");
        assert_eq!(event["unreached_after_hooks"], json!(unreached));
        assert!(!stderr.contains("terminal-secret"));
        assert!(!stderr.contains("after-secret"));
        assert!(!stderr.contains(server.directory.0.to_str().unwrap()));
    }
}

#[test]
fn ra_terminal_covers_invalid_http_before_session() {
    let _case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        "{private-invalid",
    );
    assert_eq!(response.status, 400);
    server.stop("-TERM");
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let events: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == "request_terminal")
        .collect();
    assert_eq!(events.len(), 1, "{stderr}");
    assert_eq!(events[0]["status"], 400);
    assert_eq!(events[0]["transaction"], "not_started");
    assert_eq!(events[0]["phase"], "response");
    assert!(!stderr.contains("private-invalid"));
}
