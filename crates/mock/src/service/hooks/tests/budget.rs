//! Share one budget across HTTP, hook, root, and native field work, then reset it for the next request.

use super::*;
use crate::hooks::HookSlot;

#[tokio::test]
async fn ra_scope_request_regex_failure_rolls_back_prior_root_and_both_models() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        r#"type A{words:[String!]! @constraint(pattern:"^$")}
        type B{words:[String!]! @constraint(pattern:"^$")}
        type Query{counts:[Int!]!}
        type Mutation{good:Int! bad:Int}"#,
        "{}",
        &[
            (
                "Mutation.good",
                r#"local words: {string} = {}
                for i=1,1000 do words[i] = "" end
                ctx.models.A:create({words=words})
                ctx.models.B:create({words=words})
                return 1"#,
            ),
            (
                "Mutation.bad",
                r#"local words: {string} = {}
                for i=1,900 do words[i] = "" end
                ctx.models.A:create({words=words})
                ctx.models.B:create({words=words})
                local tail: {string} = {}
                for i=1,300 do tail[i] = "" end
                pcall(function() ctx.models.A:create({words=tail}) end)
                return 2"#,
            ),
            (
                "Query.counts",
                "return {ctx.models.A:count({}),ctx.models.B:count({})}",
            ),
        ],
        &[
            (HookSlot::FieldBefore, "ctx.models.A:count({}) return nil"),
            (
                HookSlot::FieldAfter,
                r#"
                if ctx.field.coordinate == 'Mutation.good' then
                    ctx.log('info', 'before-read')
                    ctx.models.A:find({})
                    ctx.models.B:find({})
                    ctx.log('info', 'after-read')
                end
                return nil
            "#,
            ),
        ],
    );
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let work = scope.0.execution_budget.0.clone();
    let observed_work = work.clone();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let received = observed.clone();
    let scope = scope.with_events(move |event| {
        if let Ok(record) = event.record
            && matches!(record.message.as_str(), "before-read" | "after-read")
        {
            received
                .lock()
                .unwrap()
                .push((record.message, observed_work.match_count()));
        }
    });
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let http = scope
        .run(fixture.endpoint.handle(
            request,
            BufferedBody::new(br#"{"query":"mutation {first:good bad}"}"#.to_vec()),
            &Clock,
        ))
        .await;
    assert_eq!(http.status, 500);
    let response: serde_json::Value = serde_json::from_slice(&http.body).unwrap();
    assert_eq!(
        *observed.lock().unwrap(),
        vec![("before-read".into(), 2000), ("after-read".into(), 2000)]
    );
    assert_eq!(work.match_count(), 4096);
    assert_eq!(response["data"], serde_json::json!(null), "{response}");
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(response["errors"][0]["extensions"]["limit"], "regexMatches");
    let (response, _) = fixture.query("{counts}").await;
    assert_eq!(response["data"]["counts"], serde_json::json!([0, 0]));
    assert!(response.get("errors").is_none());
    let (response, _) = fixture.query("mutation {bad}").await;
    assert_eq!(response["data"]["bad"], 2, "{response}");
    assert!(response.get("errors").is_none());
    let (response, _) = fixture.query("mutation {good}").await;
    assert_eq!(response["data"]["good"], 1);
    assert!(response.get("errors").is_none());
    fixture.stop().await;
}

#[tokio::test]
async fn sd_scope_hook_copies_and_resolver_share_node_limit_without_changing_committed_rows() {
    let _case = crate::vm_gate::test_case();
    let seed = serde_json::json!({"A":[{"words":vec![""; 1000]}]}).to_string();
    let fixture = Fixture::new(
        "type A{words:[String!]!} type Query{count:Int!} type Mutation{make:Int}",
        &seed,
        &[
            ("Query.count", "return ctx.models.A:count({})"),
            (
                "Mutation.make",
                r#"ctx.models.A:create({words={"added"}})
                ctx.models.A:find({})
                ctx.models.A:find({})
                ctx.models.A:find({})
                ctx.models.A:find({})
                return 1"#,
            ),
        ],
        &[(
            HookSlot::RequestBefore,
            "ctx.models.A:find({}) ctx.models.A:find({}) return nil",
        )],
    );
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let mut work = scope.0.execution_budget.0.clone();
    // Leave enough capacity for both phases to run while avoiding a slow
    // boundary test that repeats the same real copy hundreds of times.
    work.reserve_value(95_000, 0).unwrap();
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let http = scope
        .run(fixture.endpoint.handle(
            request,
            BufferedBody::new(br#"{"query":"mutation {make}"}"#.to_vec()),
            &Clock,
        ))
        .await;
    let response: serde_json::Value = serde_json::from_slice(&http.body).unwrap();
    assert_eq!(response["data"], serde_json::json!(null));
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["errors"][0]["extensions"]["code"], "MOCK_EXECUTION_LIMIT_EXCEEDED",
        "{response}"
    );
    assert_eq!(response["errors"][0]["extensions"]["limit"], "valueNodes");
    let (response, _) = fixture.query("{count}").await;
    assert_eq!(response["data"]["count"], 1);
    assert!(response.get("errors").is_none());
    fixture.stop().await;
}

#[tokio::test]
async fn sd_scope_native_output_and_argument_defaults_do_not_restart_regex_counts() {
    let _case = crate::vm_gate::test_case();
    let words = vec!["\"\""; 500].join(",");
    let sdl = format!(
        "type A{{words:[String!]! @constraint(pattern:\"^$\")}} type Query{{rows(arg:[String!]! = [{words}] @constraint(pattern:\"^$\")): [A!]}}"
    );
    let seed = serde_json::json!({"A":[{"words":vec![""; 1000]}]}).to_string();
    let fixture = Fixture::new(
        &sdl,
        &seed,
        &[("Query.rows", "return ctx.models.A:find({})")],
        &[],
    );
    let (response, _) = fixture.query("{rows {a:words b:words c:words}}").await;
    assert_eq!(response["data"], serde_json::Value::Null);
    assert_eq!(response["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["errors"][0]["extensions"]["code"], "MOCK_EXECUTION_LIMIT_EXCEEDED",
        "{response}"
    );
    assert_eq!(response["errors"][0]["extensions"]["limit"], "regexMatches");
    assert!(response["errors"][0]["extensions"]["requestId"].is_string());
    let (response, _) = fixture.query("{rows {words}}").await;
    assert_eq!(
        response["data"]["rows"][0]["words"]
            .as_array()
            .unwrap()
            .len(),
        1000
    );
    assert!(response.get("errors").is_none());
    fixture.stop().await;
}
