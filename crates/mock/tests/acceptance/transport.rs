//! Verify loopback/CORS policy, endpoint limits, restart, and signal recovery.

use super::support::{Server, TEMPLATE, edited};
use serde_json::json;
use std::net::TcpStream;

#[test]
fn mock_15_exact_cors_preflight_and_denied_capabilities() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    let response = server.http(
        "OPTIONS",
        &[
            ("Origin", "http://localhost:5173"),
            ("Access-Control-Request-Method", "POST"),
            ("Access-Control-Request-Headers", "content-type,accept"),
        ],
        "",
    );
    assert_eq!(response.status, 204);
    assert!(
        response
            .headers
            .contains("access-control-allow-origin: http://localhost:5173")
    );
    assert!(
        response
            .headers
            .contains("access-control-allow-methods: get, post")
    );
    assert!(
        !response
            .headers
            .contains("access-control-allow-credentials")
    );
    for headers in [
        vec![
            ("Origin", "http://unknown.invalid"),
            ("Access-Control-Request-Method", "POST"),
        ],
        vec![
            ("Origin", "http://localhost:5173"),
            ("Access-Control-Request-Method", "DELETE"),
        ],
        vec![
            ("Origin", "http://localhost:5173"),
            ("Access-Control-Request-Method", "POST"),
            ("Access-Control-Request-Headers", "x-unapproved"),
        ],
        vec![
            ("Origin", "http://localhost:5173"),
            ("Origin", "http://unknown.invalid"),
            ("Access-Control-Request-Method", "POST"),
        ],
    ] {
        let response = server.http("OPTIONS", &headers, "");
        assert_eq!(response.status, 403);
        assert!(!response.headers.contains("access-control-allow-origin"));
    }
    assert_eq!(server.http("POST", &[("Origin", "null")], "{}").status, 403);
    server.stop("-TERM");
    let body = r#"
local globals = _G :: any
for _, name in {"io", "debug", "require", "load", "loadstring", "package", "print", "getfenv", "setfenv", "coroutine"} do
    if globals[name] ~= nil then error("forbidden capability") end
end
return ctx.models.User:find({limit=args.limit})
"#;
    let mut server = Server::start(&edited("Query.getUsers", body));
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
}

#[test]
fn mock_13_restart_seed_and_idle_connection_shutdown() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"new\"}){id}}")["data"]["createUser"]["id"],
        "User-3"
    );
    let idle = TcpStream::connect(server.address).unwrap();
    server.stop("-INT");
    drop(idle);
    let mut restarted = Server::start(TEMPLATE);
    assert_eq!(
        restarted.graphql("{getUsers{id}}")["data"]["getUsers"],
        json!([{"id":"User-1"},{"id":"User-2"}])
    );
    assert_eq!(
        restarted.graphql("mutation{createUser(input:{name:\"after restart\"}){id}}")["data"]["createUser"]
            ["id"],
        "User-3"
    );
    restarted.stop("-TERM");
}

#[test]
fn mock_13_shutdown_deadline_begins_at_signal_after_longer_uptime() {
    let _test_case = crate::support::test_case();
    let mut servers = [Server::start(TEMPLATE), Server::start(TEMPLATE)];
    std::thread::sleep(std::time::Duration::from_millis(5300));
    servers[0].stop("-TERM");
    servers[1].stop("-INT");
}

#[test]
fn mock_15_string_metatable_does_not_restore_hidden_library() {
    let _test_case = crate::support::test_case();
    // If hidden string methods return, a successful 200 response fails this test.
    let body = "if args.limit == 1 then pcall(function() return (\"ab\"):rep(2) end) end return ctx.models.User:find({limit=20})";
    let mut server = Server::start(&edited("Query.getUsers", body));
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"{getUsers(limit:1){id}}"}).to_string(),
    );
    assert_eq!(response.status, 500);
    let failed: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(failed["data"].is_null());
    assert_eq!(
        failed["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
}

#[test]
fn mock_07_endpoint_body_and_depth_limits_remain_active() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    assert_eq!(
        server
            .http(
                "POST",
                &[("Content-Type", "application/json")],
                &"x".repeat(1024 * 1024 + 1)
            )
            .status,
        413
    );
    let nested = format!(
        "{{getUsers{{{}id{}}}}}",
        "posts{author{".repeat(20),
        "}}".repeat(20)
    );
    let result = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":nested}).to_string(),
    );
    assert_eq!(result.status, 422);
    assert!(result.body.contains("errors"));
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
}
