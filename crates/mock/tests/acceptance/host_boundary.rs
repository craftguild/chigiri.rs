//! Verify the Rust host rejects type-check bypasses and forged storage records.

use super::support::{Directory, Server, TEMPLATE, edited, seed, with_seed};
use serde_json::json;

#[test]
fn mock_08_11_actual_host_types_keys_limits_and_record_fields() {
    let _test_case = crate::support::test_case();
    for call in [
        "ctx.models.User:find(({limit=1.5} :: any))",
        "ctx.models.User:find(({limit=\"1\"} :: any))",
        "ctx.models.User:find(({limit=true} :: any))",
        "ctx.models.User:find(({limit=0/0} :: any))",
        "ctx.models.User:find(({limit=9007199254740992} :: any))",
        "ctx.models.User:find(({limit=1, extra=1} :: any))",
        "ctx.models.Post:find(({where={field='id',op='eq',value=1}} :: any))",
        "ctx.models.Post:find(({where={field='authorId',op='eq',value=''}} :: any))",
        "(ctx.models.User.find :: any)(ctx.models.User, {}, 1)",
        "(ctx.models.User.find :: any)({}, {})",
    ] {
        let body = format!(
            "if args.limit == 1 then {call} end return ctx.models.User:find({{limit=args.limit}})"
        );
        let mut server = Server::start(&edited("Query.getUsers", &body));
        rejected(&server, "{getUsers(limit:1){id}}");
        assert_eq!(
            server.graphql("{getUsers{id}}")["data"]["getUsers"],
            json!([{"id":"User-1"},{"id":"User-2"}])
        );
        server.stop("-TERM");
    }
    for change in [
        "rows[1].authorId = \"User-2\"",
        "rows[1].body = (42 :: any)",
        "(rows[1] :: any).extra = true",
        "rows[1].id = \"invented\"",
    ] {
        let body = format!(
            "local rows = ctx.models.Post:find({{limit=args.limit}}) if args.limit == 1 then {change} end return rows"
        );
        let mut server = Server::start(&edited("Query.findPost", &body));
        rejected(&server, "{findPost(limit:1){id}}");
        assert_eq!(
            server.graphql("{findPost{id body author{id}}}")["data"]["findPost"],
            json!([{"id":"Post-1","body":"body","author":{"id":"User-1"}}])
        );
        server.stop("-TERM");
    }
}

#[test]
fn mock_08_post_capacity_seed_duplicates_and_inclusive_script_bytes() {
    let _test_case = crate::support::test_case();
    let mut records = seed(TEMPLATE);
    records["Post"] = (1..=1000)
        .map(|id| json!({"id":format!("Post-{id}"),"title":"","body":"","authorId":"User-1"}))
        .collect();
    let config = with_seed(
        &edited(
            "Mutation.createPost",
            "return ctx.models.Post:create({id='Post-1001', title=args.input.title, body=args.input.body, authorId=args.input.authorId})",
        ),
        &records,
    );
    let mut server = Server::start(&config);
    rejected(
        &server,
        "mutation{createPost(input:{title:\"\",body:\"\",authorId:\"User-1\"}){id}}",
    );
    assert_eq!(
        server.graphql("mutation{deletePost(id:\"Post-1\")}")["data"]["deletePost"],
        true
    );
    assert_eq!(
        server.graphql(
            "mutation{createPost(input:{title:\"\",body:\"\",authorId:\"User-2\"}){id author{id}}}"
        ),
        json!({"data":{"createPost":{"id":"Post-1001","author":{"id":"User-2"}}}})
    );
    assert_eq!(
        server.graphql("{findPost(limit:1000){id}}")["data"]["findPost"]
            .as_array()
            .unwrap()
            .len(),
        1000
    );
    server.stop("-TERM");
    for id in ["Post-1", "Post-1001"] {
        let directory = Directory::new();
        let mut invalid = records.clone();
        invalid["Post"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"title":"","body":"","authorId":"User-1"}));
        fs_write(&directory, &with_seed(TEMPLATE, &invalid));
        assert_eq!(
            directory
                .command(&["serve", "--config", "mock.toml"])
                .status
                .code(),
            Some(1)
        );
    }
    let mut document = edited(
        "Query.getUsers",
        "return ctx.models.User:find({limit=args.limit})",
    )
    .parse::<toml_edit::DocumentMut>()
    .unwrap();
    let mut module = document["resolvers"]["Query.getUsers"]["luau"]
        .as_str()
        .unwrap()
        .to_owned();
    module.push_str(&" ".repeat(32 * 1024 - module.len()));
    assert_eq!(module.len(), 32 * 1024);
    document["resolvers"]["Query.getUsers"]["luau"] = toml_edit::value(module);
    let mut server = Server::start(&document.to_string());
    assert_eq!(
        server.graphql("{getUsers{id}}")["data"]["getUsers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server.stop("-TERM");
}

fn fs_write(directory: &Directory, config: &str) {
    std::fs::write(directory.0.join("mock.toml"), config).unwrap();
}

/// Assert host failure masks details from HTTP and fails the entire request.
fn rejected(server: &Server, query: &str) {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":query}).to_string(),
    );
    assert_eq!(response.status, 500, "{}", response.body);
    let result: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(result["data"].is_null());
    assert_eq!(result["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(result["errors"][0]["message"], "Mock execution failed");
}
