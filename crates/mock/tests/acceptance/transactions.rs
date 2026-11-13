//! Verifies commit and rollback boundaries across all models, ordering, counters,
//! copied values, and query capability isolation.

use super::support::{Directory, Server, edited};
use serde_json::json;

fn seed(server: &Server) {
    assert_eq!(
        server.graphql("{getUsers{id name posts{id}} findPost{id title body author{id}}}")["data"],
        json!({
            "getUsers":[{"id":"User-1","name":"Alice","posts":[{"id":"Post-1"}]},{"id":"User-2","name":"Bob","posts":[]}],
            "findPost":[{"id":"Post-1","title":"first","body":"body","author":{"id":"User-1"}}]
        })
    );
}

#[test]
fn mock_09_error_and_invalid_return_restore_both_models_order_and_counters() {
    let _test_case = crate::support::test_case();
    for finish in ["error(\"private-script-secret\")", "return (42 :: any)"] {
        let body = format!(
            r#"
local user = ctx.models.User:create(args.input)
if args.input.name == "fail" then
    ctx.models.Post:create({{title="temporary",body="temporary",authorId=user.id}})
    ctx.models.Post:update("Post-1", {{ title="temporary", authorId=user.id }})
    ctx.models.Post:delete("Post-1")
    {finish}
end
return user
"#
        );
        let mut server = Server::start(&edited("Mutation.createUser", &body));
        let result = failed(&server, "mutation{createUser(input:{name:\"fail\"}){id}}");
        assert!(result["errors"].is_array(), "{result}");
        assert!(result["data"].is_null());
        assert!(!result.to_string().contains("private-script-secret"));
        seed(&server);
        assert_eq!(
            server.graphql("mutation{createUser(input:{name:\"ok\"}){id}}")["data"]["createUser"]["id"],
            "User-3"
        );
        assert_eq!(
            server.graphql(
                "mutation{createPost(input:{title:\"\",body:\"\",authorId:\"User-2\"}){id}}"
            )["data"]["createPost"]["id"],
            "Post-2"
        );
        assert_eq!(
            server.graphql("{findPost{id}}")["data"]["findPost"],
            json!([{"id":"Post-1"},{"id":"Post-2"}])
        );
        server.stop("-TERM");
    }
}

#[test]
fn mock_10_multimodel_commit_caught_host_failure_and_request_rollback() {
    let _test_case = crate::support::test_case();
    let body = r#"
local user = ctx.models.User:create(args.input)
if args.input.name == "fail" then
    pcall(function() ctx.models.Post:create({title="",body="",authorId="missing"}) end)
else
    ctx.models.Post:create({title="from script",body="",authorId=user.id})
end
return user
"#;
    let mut server = Server::start(&edited("Mutation.createUser", body));
    failed(&server, "mutation{createUser(input:{name:\"fail\"}){id}}");
    seed(&server);
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"ok\"}){id posts{id author{id}}}}")["data"]
            ["createUser"],
        json!({"id":"User-3","posts":[{"id":"Post-2","author":{"id":"User-3"}}]})
    );
    let result = failed(
        &server,
        "mutation{first:createUser(input:{name:\"one\"}){id} second:createUser(input:{name:\"fail\"}){id} third:createUser(input:{name:\"three\"}){id}}",
    );
    assert!(result["errors"].is_array());
    assert!(result["data"].is_null());
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"],
        json!([{"id":"User-1"},{"id":"User-2"},{"id":"User-3"}])
    );
    assert_eq!(
        server.graphql("{findPost{id}}")["data"]["findPost"],
        json!([{"id":"Post-1"},{"id":"Post-2"}])
    );
    // IDs consumed by the failed request are restored for reuse by the next request.
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"recovered\"}){id posts{id}}}")["data"]["createUser"],
        json!({"id":"User-4","posts":[{"id":"Post-3"}]})
    );
    server.stop("-TERM");
}

#[test]
fn mock_11_query_static_dynamic_write_and_fake_or_modified_copies() {
    let _test_case = crate::support::test_case();
    let directory = Directory::new();
    std::fs::write(
        directory.0.join("mock.toml"),
        edited(
            "Query.getUsers",
            "return {ctx.models.User:create({name=\"x\"})}",
        ),
    )
    .unwrap();
    let rejected = directory.command(&["serve", "--config", "mock.toml"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("[type]"));
    for body in [
        "local rows = ctx.models.User:find({limit=args.limit}) if args.limit == 1 then (ctx.models.User :: any).create(ctx.models.User, {name=\"x\"}) end return rows",
        "local rows = ctx.models.User:find({limit=args.limit}) if args.limit == 1 then rows[1].name = \"changed-copy\" end return rows",
        "local rows = ctx.models.User:find({limit=args.limit}) if args.limit == 1 then rows[1].id = \"invented\" end return rows",
    ] {
        let mut server = Server::start(&edited("Query.getUsers", body));
        failed(&server, "{getUsers(limit:1){id name}}");
        seed(&server);
        server.stop("-TERM");
    }
}

/// Assert that execution failure returns no data.
fn failed(server: &Server, query: &str) -> serde_json::Value {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query": query}).to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let result: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(result["data"].is_null());
    assert_eq!(
        result["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(result["errors"][0]["message"], "Mock execution failed");
    result
}
