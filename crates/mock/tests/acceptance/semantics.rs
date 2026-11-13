//! Verify search, ordering, relation, and existing GraphQL executor semantics over HTTP.

use super::support::{Server, TEMPLATE};
use serde_json::json;

#[test]
fn mock_05_find_and_noop_missing_update_and_delete() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    for (title, author) in [("second", "User-2"), ("third", "User-1")] {
        assert!(server.operation(json!({"query": "mutation($t:String!,$a:ID!){createPost(input:{title:$t,body:\"\",authorId:$a}){id}}", "variables":{"t":title,"a":author}}))["errors"].is_null());
    }
    for (query, expected) in [
        (
            "{findPost{id}}",
            json!([{"id":"Post-1"},{"id":"Post-2"},{"id":"Post-3"}]),
        ),
        ("{findPost(id:\"Post-2\"){id}}", json!([{"id":"Post-2"}])),
        (
            "{findPost(id:\"Post-2\",authorId:\"User-1\"){id}}",
            json!([]),
        ),
        (
            "{findPost(authorId:\"User-2\",limit:1){id}}",
            json!([{"id":"Post-2"}]),
        ),
        ("{findPost(id:null,authorId:null,limit:0){id}}", json!([])),
        (
            "{findPost(id:null,authorId:null,limit:100){id}}",
            json!([{"id":"Post-1"},{"id":"Post-2"},{"id":"Post-3"}]),
        ),
    ] {
        assert_eq!(
            server.graphql(query)["data"]["findPost"],
            expected,
            "{query}"
        );
    }
    assert_eq!(
        server.graphql("mutation{updatePost(id:\"Post-2\",input:{}){id title body author{id}}}")["data"]
            ["updatePost"],
        json!({"id":"Post-2","title":"second","body":"","author":{"id":"User-2"}})
    );
    // UpdatePost is non-null in the fixture, so returning no object fails the entire request.
    let missing = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{updatePost(id:\"absent\",input:{}){id}}"}).to_string(),
    );
    assert_eq!(missing.status, 500, "{}", missing.body);
    let missing: serde_json::Value = serde_json::from_str(&missing.body).unwrap();
    assert!(missing["data"].is_null());
    assert_eq!(missing["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        missing["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(
        server.graphql("mutation{deletePost(id:\"absent\")}")["data"]["deletePost"],
        false
    );
    assert_eq!(
        server.graphql("{findPost{id}}")["data"]["findPost"],
        json!([{"id":"Post-1"},{"id":"Post-2"},{"id":"Post-3"}])
    );
    server.stop("-TERM");
}

#[test]
fn mock_06_both_relations_follow_only_author_id_and_registration_order() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    for author in ["User-1", "User-2"] {
        server.operation(json!({"query":"mutation($a:ID!){createPost(input:{title:\"\",body:\"\",authorId:$a}){id}}", "variables":{"a":author}}));
    }
    assert_eq!(
        server.graphql("{getUsers{id posts{id}}}")["data"]["getUsers"],
        json!([
            {"id":"User-1","posts":[{"id":"Post-1"},{"id":"Post-2"}]}, {"id":"User-2","posts":[{"id":"Post-3"}]}
        ])
    );
    server.graphql("mutation{updatePost(id:\"Post-2\",input:{authorId:\"User-2\"}){id}}");
    assert_eq!(
        server.graphql("{getUsers{id posts{id author{id}}}}")["data"]["getUsers"],
        json!([
            {"id":"User-1","posts":[{"id":"Post-1","author":{"id":"User-1"}}]},
            {"id":"User-2","posts":[{"id":"Post-3","author":{"id":"User-2"}},{"id":"Post-2","author":{"id":"User-2"}}]}
        ])
    );
    server.graphql("mutation{deletePost(id:\"Post-2\")}");
    assert_eq!(
        server.graphql("{findPost{id} getUsers{id posts{id}}}")["data"],
        json!({
            "findPost":[{"id":"Post-1"},{"id":"Post-3"}],
            "getUsers":[{"id":"User-1","posts":[{"id":"Post-1"}]},{"id":"User-2","posts":[{"id":"Post-3"}]}]
        })
    );
    server.stop("-TERM");
}

#[test]
fn mock_07_variables_alias_fragment_selection_coercion_and_null_propagation() {
    let _test_case = crate::support::test_case();
    let mut server = Server::start(TEMPLATE);
    let result = server.operation(json!({"query":"query Read($a:ID!, $n:Int! = 1){ selected:findPost(authorId:$a,limit:$n){...P} } fragment P on Post { label:title author { name posts { id } } }", "operationName":"Read", "variables":{"a":"User-1"}}));
    assert_eq!(
        result["data"],
        json!({"selected":[{"label":"first","author":{"name":"Alice","posts":[{"id":"Post-1"}]}}]})
    );
    for operation in [
        json!({"query":"{getUsers{unknown}}"}),
        json!({"query":"query($n:Int!){getUsers(limit:$n){id}}","variables":{"n":1.5}}),
        json!({"query":"query($n:Int!){getUsers(limit:$n){id}}","variables":{"n":"1"}}),
        json!({"query":"query($n:Int!){getUsers(limit:$n){id}}","variables":{}}),
        json!({"query":"mutation{createUser(input:{name:\"x\",id:\"injected\"}){id}}"}),
    ] {
        let response = server.http(
            "POST",
            &[("Content-Type", "application/json")],
            &operation.to_string(),
        );
        assert_eq!(response.status, 422);
        let result: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert!(result["errors"].is_array());
        assert!(result["data"].is_null());
    }
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        r#"{"query":"{ getUsers { id posts(limit:-1) { id } } }"}"#,
    );
    assert_eq!(response.status, 422);
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
}
