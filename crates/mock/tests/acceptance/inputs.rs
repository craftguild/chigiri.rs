//! Verify MOCK-08 omission/null handling, UTF-8 byte limits, capacity, and reference boundaries.

use super::support::{Server, TEMPLATE, seed, with_seed};
use serde_json::{Value, json};

#[test]
fn mock_08_null_limits_ids_lengths_and_unknown_authors() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    for field in ["title", "body", "authorId"] {
        let result = rejected(
            &server,
            json!({"query":"mutation($i:UpdatePostInput!){updatePost(id:\"Post-1\",input:$i){id}}","variables":{"i":{field:Value::Null}}}),
        );
        assert!(result["errors"].is_array(), "{field}: {result}");
        assert!(result["data"].is_null());
    }
    for query in [
        "{getUsers(limit:-1){id}}",
        "mutation{updatePost(id:\"Post-1\",input:{authorId:\"missing\"}){id}}",
        "mutation{createPost(input:{title:\"\",body:\"\",authorId:\"missing\"}){id}}",
    ] {
        rejected(&server, json!({"query":query}));
    }
    for (name, succeeds) in [
        ("".into(), true),
        ("é".repeat(128), true),
        ("é".repeat(129), true),
    ] {
        let result = server.operation(json!({"query":"mutation($n:String!){createUser(input:{name:$n}){id name}}","variables":{"n":name}}));
        assert_eq!(result["errors"].is_null(), succeeds, "{result}");
    }
    for (field, size) in [("title", 257), ("body", 4097), ("authorId", 257)] {
        let result = rejected(
            &server,
            json!({"query":"mutation($i:UpdatePostInput!){updatePost(id:\"Post-1\",input:$i){id}}", "variables":{"i":{field:"x".repeat(size)}}}),
        );
        assert!(result["errors"].is_array());
    }
    rejected(
        &server,
        json!({"query":"query($i:ID){findPost(id:$i){id}}", "variables":{"i":"x".repeat(257)}}),
    );
    for query in ["{findPost(id:\"\"){id}}", "mutation{deletePost(id:\"\")}"] {
        rejected(&server, json!({"query":query}));
    }
    let result = server.operation(json!({"query":"mutation($i:UpdatePostInput!){updatePost(id:\"Post-1\",input:$i){title body}}", "variables":{"i":{"title":"x".repeat(256),"body":"x".repeat(4096)}}}));
    assert_eq!(
        result["data"]["updatePost"]["title"]
            .as_str()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(
        result["data"]["updatePost"]["body"].as_str().unwrap().len(),
        4096
    );
    server.stop("-TERM");
}

#[test]
fn mock_08_seed_capacity_and_seed_collision_safe_ids() {
    let _test_case = crate::support::test_case();
    let mut records = seed(TEMPLATE);
    records["User"] = (1..=1000)
        .map(|id| json!({"id":format!("User-{id}"),"name":""}))
        .collect();
    let config = with_seed(TEMPLATE, &records);
    let mut server = Server::start(&config);
    rejected(
        &server,
        json!({"query":"mutation{createUser(input:{name:\"x\"}){id}}"}),
    );
    assert_eq!(
        server.graphql("{getUsers(limit:100){id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
    server.stop("-TERM");
    let mut server = Server::start(TEMPLATE);
    let result = server.graphql(
        "mutation{a:createUser(input:{name:\"\"}){id} b:createUser(input:{name:\"\"}){id}}",
    );
    let first = result["data"]["a"]["id"].as_str().unwrap();
    let second = result["data"]["b"]["id"].as_str().unwrap();
    assert_ne!(first, second);
    server.stop("-TERM");
}

/// Storage limit, capacity, and reference violations fail the entire request safely.
fn rejected(server: &Server, operation: Value) -> Value {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &operation.to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert!(result["data"].is_null());
    assert_eq!(result["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    result
}
