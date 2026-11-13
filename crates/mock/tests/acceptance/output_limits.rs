//! Verify response byte/node limits and nested-failure rollback at the real endpoint.

use super::support::{Server, TEMPLATE, edited, seed, with_seed};
use serde_json::{Value, json};

#[test]
fn mock_07_large_relation_output_hits_request_work_limit() {
    let _test_case = crate::support::test_case();
    let mut records = seed(TEMPLATE);
    records["Post"] = (1..=20).map(|id| json!({"id":format!("Post-{id}"),"title":"","body":"x".repeat(4096),"authorId":"User-1"})).collect();
    let config = with_seed(TEMPLATE, &records);
    assert!(config.len() < 256 * 1024);
    let mut server = Server::start(&config);
    // This relationship extension reaches the shared work limit before the encode limit.
    for query in [
        "{getUsers(limit:1){posts{author{posts{body}}}}}",
        "{getUsers(limit:1){posts{author{posts{author{posts{id body}}}}}}}",
    ] {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &json!({"query":query}).to_string(),
        );
        assert_eq!(response.status, 500, "{}", response.body);
        assert!(response.body.len() < 1024);
        let encoded: Value = serde_json::from_str(&response.body).unwrap();
        assert!(encoded["data"].is_null());
        assert_eq!(encoded["errors"].as_array().unwrap().len(), 1);
        assert_eq!(
            encoded["errors"][0]["extensions"]["code"],
            "MOCK_EXECUTION_LIMIT_EXCEEDED"
        );
        assert_eq!(encoded["errors"][0]["extensions"]["limit"], "work");
    }
    assert_eq!(
        server.graphql("{findPost(limit:100){id}}")["data"]["findPost"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    server.stop("-TERM");
    let stderr = std::fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    let terminal: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["kind"] == "request_terminal")
        .collect();
    assert_eq!(terminal.len(), 3, "{stderr}");
    for event in &terminal[..2] {
        assert_eq!(event["status"], 500);
        assert_eq!(event["transaction"], "rolled_back");
        assert_eq!(event["cause"], "hard_limit");
        assert_eq!(event["limit"]["name"], "work");
        assert!(
            event["limit"]["observed"].as_u64().unwrap() > event["limit"]["hard"].as_u64().unwrap()
        );
    }
    assert_eq!(terminal[2]["transaction"], "committed");
    assert_eq!(terminal[2]["cause"], "success");
}

#[test]
fn mock_09_nested_completion_error_rolls_back_root_candidate() {
    let _test_case = crate::support::test_case();
    let (_, config) = super::runtime_schema::init(
        "type User{id:ID! name:String! posts:[Post!]! @virtual} type Post{id:ID!} input New{name:String!} type Query{getUsers:[User!]!} type Mutation{createUser(input:New!):User!}",
    );
    let config = super::runtime_schema::ready(
        &config,
        &[
            ("Query.getUsers", "return ctx.models.User:find({})"),
            (
                "Mutation.createUser",
                "return ctx.models.User:create({id='User-' .. (ctx.models.User:count({}) + 1), name=args.input.name})",
            ),
            (
                "User.posts",
                "error('intentional nested failure before request commit')",
            ),
        ],
    );
    let config = with_seed(
        &config,
        &json!({"User":[{"id":"User-1","name":"one"},{"id":"User-2","name":"two"}]}),
    );
    let mut server = Server::start(&config);
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{createUser(input:{name:\"discarded\"}){id posts{id}}}"})
            .to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(result["data"], Value::Null);
    assert_eq!(result["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(result["errors"][0]["message"], "Mock execution failed");
    assert_eq!(
        server.graphql("{getUsers{id name}}")["data"]["getUsers"],
        json!([{"id":"User-1","name":"one"},{"id":"User-2","name":"two"}])
    );
    assert_eq!(
        server.graphql("mutation{createUser(input:{name:\"next\"}){id}}")["data"]["createUser"]["id"],
        "User-3"
    );
    server.stop("-TERM");
}

#[test]
fn mock_14_edited_update_changes_saved_value() {
    let _test_case = crate::support::test_case();
    let body = "local patch = args.input if type(patch.title) == 'string' then patch.title = \"edited:\" .. patch.title end return ctx.models.Post:update({where={field='id',op='eq',value=args.id}}, patch)[1]";
    let mut server = Server::start(&edited("Mutation.updatePost", body));
    let updated =
        server.graphql("mutation{updatePost(id:\"Post-1\",input:{title:\"new\"}){id title}}");
    assert_eq!(
        updated["data"]["updatePost"],
        json!({"id":"Post-1","title":"edited:new"})
    );
    assert_eq!(
        server.graphql("{findPost(id:\"Post-1\"){id title}}")["data"]["findPost"],
        json!([{"id":"Post-1","title":"edited:new"}])
    );
    server.stop("-TERM");
}
