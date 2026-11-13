//! End-to-end stateful authoring, private storage, and transaction acceptance.

use std::fs;

use serde_json::{Value, json};

use crate::stateful_domain_fixture::build;
use crate::stateful_domain_support::{
    assert_balances_and_history, assert_retry_contract, authoring_view, business, data, me,
    operation, transfer, upload, wait_for_diagnostic,
};
use crate::support::{Directory, Server};

#[test]
fn sa_13_14_15_16_stateful_project_executes_and_rolls_back_as_one_unit() {
    let _case = crate::support::test_case();
    let directory = build();
    let target = directory.command(&[
        "check",
        "--config-dir",
        "mock",
        "--target",
        "seeds/MockChallenge.jsonl",
        "--format",
        "json",
    ]);
    assert!(
        target.status.success(),
        "{}",
        String::from_utf8_lossy(&target.stderr)
    );
    let target: Value = serde_json::from_slice(&target.stdout).unwrap();
    assert_eq!(target["readiness_checked"], false);
    assert!(
        directory
            .command(&["check", "--config-dir", "mock"])
            .status
            .success()
    );
    assert!(
        directory
            .command(&[
                "export",
                "--config-dir",
                "mock",
                "--output",
                "portable.toml"
            ])
            .status
            .success()
    );
    let portable = fs::read_to_string(directory.0.join("portable.toml")).unwrap();
    assert!(portable.contains("format = \"chigiri-mock/v11\""));
    assert!(portable.contains("mode = \"fixed\""));
    assert!(portable.contains("[storage]"));
    assert!(portable.contains("type MockChallenge"));
    assert!(!portable.contains("$ref"));

    let project = directory.0.join("mock");
    let hidden_project = directory.0.join("mock.unavailable");
    let schema = directory.0.join("schema.graphql");
    let hidden_schema = directory.0.join("schema.unavailable");
    fs::rename(&project, &hidden_project).unwrap();
    fs::rename(&schema, &hidden_schema).unwrap();
    assert_portable_behavior(&portable);
    fs::rename(&hidden_project, &project).unwrap();
    fs::rename(&hidden_schema, &schema).unwrap();

    let mut server = Server::project_with_graphiql(directory);
    let invalid = operation(
        &server,
        None,
        r#"mutation{verify(phone:"111",purpose:"WRONG",code:"111111")}"#,
        &[],
    );
    business(&invalid, "INVALID_CODE");
    let expired = operation(
        &server,
        None,
        r#"mutation{verify(phone:"111",purpose:"RECOVERY",code:"333333")}"#,
        &[],
    );
    business(&expired, "CODE_EXPIRED");
    let exact = operation(
        &server,
        None,
        r#"mutation{verify(phone:"111",purpose:"EXACT",code:"444444")}"#,
        &[],
    );
    business(&exact, "CODE_EXPIRED");
    let first = data(
        &operation(
            &server,
            None,
            r#"mutation{verify(phone:"111",purpose:"LOGIN",code:"111111")}"#,
            &[],
        ),
        "verify",
    )
    .as_str()
    .unwrap()
    .to_owned();
    let second = data(
        &operation(
            &server,
            None,
            r#"mutation{verify(phone:"222",purpose:"LOGIN",code:"222222")}"#,
            &[],
        ),
        "verify",
    )
    .as_str()
    .unwrap()
    .to_owned();
    assert_ne!(first, second);
    assert!(!first.contains("u1") && !second.contains("u2"));
    business(
        &operation(
            &server,
            None,
            r#"mutation{verify(phone:"111",purpose:"LOGIN",code:"111111")}"#,
            &[],
        ),
        "CODE_CONSUMED",
    );
    assert_eq!(
        me(&server, &first),
        json!({"id":"u1","name":"Ada","wallet":{"id":"w1","balance":100}})
    );
    assert_eq!(
        me(&server, &second),
        json!({"id":"u2","name":"Ben","wallet":{"id":"w2","balance":20}})
    );
    let initial_view = authoring_view(&server, &first, "viewer-a");
    assert_eq!(
        initial_view,
        json!({
            "me": {
                "id":"u1",
                "images":[{"id":"i1","url":"one.png","viewerLabel":"viewer-a:u1"}],
                "featuredImage":{"id":"i1","url":"one.png","viewerLabel":"viewer-a:u1"},
                "wallet":{"id":"w1","balance":100,"rewards":{"edges":[{"node":{"id":"r1","post":{"id":"p1"}}}]}}
            },
            "feed":{"edges":[{"node":{"id":"p1","reward":{"id":"r1"}}}]}
        })
    );
    business(
        &operation(
            &server,
            Some(&second),
            "mutation{renameImage(id:\"i1\",url:\"forbidden.png\"){id}}",
            &[],
        ),
        "NOT_OWNER",
    );
    assert_eq!(
        data(
            &operation(
                &server,
                Some(&first),
                "mutation{renameImage(id:\"i1\",url:\"renamed.png\"){id url viewerLabel}}",
                &[("x-viewer", "viewer-a")],
            ),
            "renameImage",
        ),
        &json!({"id":"i1","url":"renamed.png","viewerLabel":"viewer-a:rotated-u1"})
    );
    let changed_view = authoring_view(&server, &first, "viewer-a");
    assert_eq!(changed_view["me"]["images"], initial_view["me"]["images"]);
    assert_eq!(
        changed_view["me"]["featuredImage"],
        json!({"id":"i1","url":"renamed.png","viewerLabel":"viewer-a:rotated-u1"})
    );

    let created = operation(
        &server,
        Some(&first),
        "mutation{createUpload(size:12){id ownerId declaredSize completed myVote}}",
        &[],
    );
    assert_eq!(
        data(&created, "createUpload"),
        &json!({"id":"upload-1","ownerId":"u1","declaredSize":12,"completed":false,"myVote":null})
    );
    data(
        &operation(
            &server,
            Some(&first),
            "mutation{vote(uploadId:\"upload-1\",value:-1){id}}",
            &[],
        ),
        "vote",
    );
    data(
        &operation(
            &server,
            Some(&second),
            "mutation{vote(uploadId:\"upload-1\",value:1){id}}",
            &[],
        ),
        "vote",
    );
    assert_eq!(upload(&server, &first)["myVote"], -1);
    assert_eq!(upload(&server, &second)["myVote"], 1);
    business(
        &operation(
            &server,
            Some(&second),
            "mutation{completeUpload(id:\"upload-1\"){id}}",
            &[],
        ),
        "NOT_OWNER",
    );
    let complete = operation(
        &server,
        Some(&first),
        "mutation{completeUpload(id:\"upload-1\"){completed declaredSize ownerId}}",
        &[],
    );
    assert_eq!(
        data(&complete, "completeUpload"),
        &json!({"completed":true,"declaredSize":12,"ownerId":"u1"})
    );

    let first_transfer = transfer(&server, &first, "k1", 30, &[]);
    assert_eq!(first_transfer, json!({"id":"transfer-1","amount":30}));
    assert_eq!(transfer(&server, &first, "k1", 30, &[]), first_transfer);
    business(
        &operation(
            &server,
            Some(&first),
            "mutation{transfer(toId:\"u2\",amount:31,key:\"k1\"){id}}",
            &[],
        ),
        "IDEMPOTENCY_CONFLICT",
    );
    assert_balances_and_history(&server, &first, &second, 70, 50, 1);

    let later = operation(
        &server,
        Some(&first),
        "mutation{a:transfer(toId:\"u2\",amount:5,key:\"rollback-root\"){id} b:failProvenance{id url}}",
        &[],
    );
    assert!(later.status == 500 && later.value["data"].is_null());
    assert_balances_and_history(&server, &first, &second, 70, 50, 1);
    assert_retry_contract(
        &server,
        &first,
        &second,
        "rollback-root",
        "transfer-2",
        (65, 55),
        2,
    );
    let after = operation(
        &server,
        Some(&first),
        "mutation{transfer(toId:\"u2\",amount:5,key:\"rollback-hook\"){id}}",
        &[("x-after-fail", "yes")],
    );
    assert!(after.status == 500 && after.value["data"].is_null());
    assert_balances_and_history(&server, &first, &second, 65, 55, 2);
    assert_retry_contract(
        &server,
        &first,
        &second,
        "rollback-hook",
        "transfer-3",
        (60, 60),
        3,
    );

    let introspection = server.graphql("{user:__type(name:\"User\"){fields{name}} image:__type(name:\"Image\"){fields{name}} private:__type(name:\"MockSession\"){name}}");
    assert!(introspection["data"]["private"].is_null());
    let fields = introspection["data"]["user"]["fields"].as_array().unwrap();
    assert!(!fields.iter().any(|field| field["name"] == "secret"));
    let fields = introspection["data"]["image"]["fields"].as_array().unwrap();
    assert!(!fields.iter().any(|field| field["name"] == "ownerId"));
    server.stop("-TERM");
    let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
    assert!(!stderr.contains("PRIVATE_VALUE"));

    let directory = Directory(server.directory.0.clone());
    let mut restarted = Server::project(directory);
    let token = login(&restarted, "111", "111111");
    let projection = operation(
        &restarted,
        Some(&token),
        "mutation{failProvenance{id url}}",
        &[],
    );
    assert!(projection.status == 500 && projection.value["data"].is_null());
    assert_eq!(
        projection.value["errors"][0]["message"],
        "Mock execution failed"
    );
    let projection_request = projection.value["errors"][0]["extensions"]["requestId"]
        .as_str()
        .unwrap();
    let diagnostic = wait_for_diagnostic(&restarted, "Mutation.failProvenance", projection_request);
    assert_eq!(diagnostic["cause"], "MOCK_RECORD_PROVENANCE");
    assert_eq!(
        diagnostic["module"],
        "resolvers/mutation/failProvenance.luau"
    );
    assert_eq!(diagnostic["line"], Value::Null);
    assert_eq!(diagnostic["column"], Value::Null);
    assert_eq!(
        me(&restarted, &token),
        json!({"id":"u1","name":"Ada","wallet":{"id":"w1","balance":100}})
    );
    assert_eq!(
        authoring_view(&restarted, &token, "restart")["me"]["images"],
        json!([{"id":"i1","url":"one.png","viewerLabel":"restart:u1"}])
    );
    assert!(
        data(
            &operation(&restarted, Some(&token), "{history{id}}", &[]),
            "history"
        )
        .as_array()
        .unwrap()
        .is_empty()
    );
    restarted.stop("-TERM");
    let stderr = fs::read_to_string(restarted.directory.0.join("stderr.log")).unwrap();
    assert!(!stderr.contains("PRIVATE_VALUE"));
}

fn login(server: &Server, phone: &str, code: &str) -> String {
    data(
        &operation(
            server,
            None,
            &format!("mutation{{verify(phone:\"{phone}\",purpose:\"LOGIN\",code:\"{code}\")}}"),
            &[],
        ),
        "verify",
    )
    .as_str()
    .unwrap()
    .to_owned()
}

fn assert_portable_behavior(portable: &str) {
    let mut server = Server::offline(portable);
    business(
        &operation(
            &server,
            None,
            r#"mutation{verify(phone:"111",purpose:"EXACT",code:"444444")}"#,
            &[],
        ),
        "CODE_EXPIRED",
    );
    let token = login(&server, "111", "111111");
    assert!(!token.contains("u1"));
    assert_eq!(
        me(&server, &token),
        json!({"id":"u1","name":"Ada","wallet":{"id":"w1","balance":100}})
    );
    assert_eq!(
        authoring_view(&server, &token, "portable")["me"]["images"],
        json!([{"id":"i1","url":"one.png","viewerLabel":"portable:u1"}])
    );
    server.stop("-TERM");
}
