//! Use real debug/release binaries to verify re-entry depth, rollback, and same-server recovery.

use super::support::{Server, edited};
use serde_json::{Value, json};

const RECURSIVE: &str = r#"
if args.limit == 1 then
    local function f(): ()
        pcall(f)
    end
    f()
end
return ctx.models.User:find({ limit = args.limit })
"#;

fn rejected(server: &Server, query: &str, limit: Option<&str>) -> Value {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":query}).to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let response: Value = serde_json::from_str(&response.body).unwrap();
    assert!(response["data"].is_null(), "{response}");
    let (code, message) = if limit.is_some() {
        (
            "MOCK_EXECUTION_LIMIT_EXCEEDED",
            "Mock execution limit exceeded",
        )
    } else {
        ("MOCK_EXECUTION_FAILED", "Mock execution failed")
    };
    assert_eq!(
        response["errors"][0]["extensions"]["code"], code,
        "{response}"
    );
    if let Some(limit) = limit {
        assert_eq!(response["errors"][0]["extensions"]["limit"], limit);
    }
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    assert_eq!(response["errors"][0]["message"], message);
    response
}

fn seed(server: &Server) {
    assert_eq!(
        server.graphql("{getUsers{id name posts{id}} findPost{id title body author{id}}}")["data"],
        json!({
            "getUsers":[
                {"id":"User-1","name":"Alice","posts":[{"id":"Post-1"}]},
                {"id":"User-2","name":"Bob","posts":[]}
            ],
            "findPost":[{"id":"Post-1","title":"first","body":"body","author":{"id":"User-1"}}]
        })
    );
}

#[test]
fn mock_12_recursive_pcall_rejects_and_same_server_recovers() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(&edited("Query.getUsers", RECURSIVE));
    let response = rejected(
        &server,
        "{ getUsers(limit: 1) { id } }",
        Some("pcallReentry"),
    );
    println!("recursive Query: {response}");
    seed(&server);
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"ok\"}){id}}")["data"]["createUser"]["id"],
        "User-3"
    );
    server.stop("-TERM");
}

#[test]
fn mock_12_caught_pcall_failures_restore_both_models_order_and_counters() {
    let _test_case = crate::support::test_case();
    for (case, failure) in [
        ("reentry", "local function f(): () pcall(f) end pcall(f)"),
        (
            "memory",
            "pcall(function() local s = \"x\" for i = 1, 30 do s = s .. s end end)",
        ),
    ] {
        let body = format!(
            r#"
local user = ctx.models.User:create(args.input)
ctx.models.Post:create({{title="temporary", body="body", authorId=user.id}})
if args.input.name == "fail" then
    ctx.models.Post:update("Post-1", {{title="changed", authorId=user.id}})
    ctx.models.Post:delete("Post-1")
    {failure}
end
return user
"#
        );
        let mut server = Server::start(&edited("Mutation.createUser", &body));
        let response = rejected(
            &server,
            "mutation{createUser(input:{name:\"fail\"}){id}}",
            Some(if case == "reentry" {
                "pcallReentry"
            } else {
                "vmMemoryBytes"
            }),
        );
        println!("caught {case}: {response}");
        seed(&server);
        for (name, user, post) in [
            ("first", "User-3", "Post-2"),
            ("second", "User-4", "Post-3"),
        ] {
            assert_eq!(
                server.graphql(&format!(
                    "mutation{{createUser(input:{{name:\"{name}\"}}){{id posts{{id author{{id}}}}}}}}"
                ))["data"]["createUser"],
                json!({"id":user,"posts":[{"id":post,"author":{"id":user}}]}),
                "{case}"
            );
        }
        assert_eq!(
            server.graphql("{getUsers{id} findPost{id}}")["data"],
            json!({
                "getUsers":[{"id":"User-1"},{"id":"User-2"},{"id":"User-3"},{"id":"User-4"}],
                "findPost":[{"id":"Post-1"},{"id":"Post-2"},{"id":"Post-3"}]
            })
        );
        server.stop("-TERM");
    }
}

#[test]
fn mock_12_pcall_depth_is_concurrent_and_caught_errors_abort_request() {
    let _test_case = crate::support::test_case();
    let body = r#"
local function descend(n: number): number
    if n == 0 then return 7 end
    local ok, value = pcall(descend, n - 1)
    assert(ok)
    return value
end
local function failing(n: number): ()
    if n == 0 then error("expected") end
    local ok = pcall(failing, n - 1)
    if n == 1 then assert(not ok) else assert(ok) end
end
for i = 1, 4 do
    assert(descend(args.limit) == 7)
    if args.limit == 0 then failing(16) end
end
return ctx.models.User:find({limit=20})
"#;
    let mut server = Server::start(&edited("Query.getUsers", body));
    for limit in [1, 16] {
        assert_eq!(
            server.graphql(&format!("{{getUsers(limit:{limit}){{id}}}}"))["data"]["getUsers"],
            json!([{"id":"User-1"},{"id":"User-2"}])
        );
    }
    rejected(&server, "{getUsers(limit:0){id}}", None);
    rejected(&server, "{getUsers(limit:17){id}}", Some("pcallReentry"));
    assert_eq!(
        server.graphql("{getUsers(limit:16){id}}")["data"]["getUsers"],
        json!([{"id":"User-1"},{"id":"User-2"}])
    );
    server.stop("-TERM");
}
