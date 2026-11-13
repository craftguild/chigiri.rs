//! Interleave another request around field hooks to verify snapshot stability and mutation linearity.

use super::scheduling::{Pause, Schedule, Stage};
use super::*;
use HookSlot::*;
use serde_json::json;

const BLOG: &str = include_str!("../../../../fixtures/blog.graphql");
const SEED: &str = r#"{"User":[{"id":"U","name":"original"}]}"#;
const HOOKS: &[(HookSlot, &str)] = &[
    (
        RequestBefore,
        "ctx.log('info','request',{count=ctx.models.User:count({})})",
    ),
    (
        OperationBefore,
        "ctx.log('info','operation',{count=ctx.models.User:count({})})",
    ),
    (
        FieldBefore,
        "ctx.log('info','before',{count=ctx.models.User:count({}),field=ctx.field})",
    ),
    (
        FieldAfter,
        "ctx.log('info','after',{count=ctx.models.User:count({}),field=ctx.field,result=ctx.result})",
    ),
    (
        OperationAfter,
        "ctx.log('info','operation',{count=ctx.models.User:count({})})",
    ),
    (
        RequestAfter,
        "ctx.log('info','request',{count=ctx.models.User:count({})})",
    ),
];

#[tokio::test]
async fn hk_11_concurrent_commits_do_not_replace_field_before_resolver_or_after_snapshot() {
    let _case = crate::vm_gate::test_case();
    let before = Pause::new("held", Stage::BeforeResolver);
    let after = Pause::new("held", Stage::AfterResolver);
    let operation_before = Pause::new("Held", Stage::BeforeOperation);
    let operation_after = Pause::new("Held", Stage::AfterOperation);
    let fixture = Fixture::new(BLOG, SEED, &[], HOOKS).scheduled(Schedule::new(&[
        before.clone(),
        after.clone(),
        operation_before.clone(),
        operation_after.clone(),
    ]));
    let (held, ()) = tokio::join!(
        fixture.query("query Held { held:getUsers { id name } }"),
        async {
            operation_before.reached().await;
            create(&fixture, "phase").await;
            operation_before.proceed().await;
            before.reached().await;
            create(&fixture, "second").await;
            before.proceed().await;
            after.reached().await;
            create(&fixture, "third").await;
            after.proceed().await;
            operation_after.reached().await;
            create(&fixture, "request").await;
            operation_after.proceed().await;
        }
    );
    assert_eq!(
        held,
        (
            json!({"data":{"held":[{"id":"U","name":"original"},{"id":"User-2","name":"phase"}]}}),
            None
        )
    );
    let records = records(&fixture, "held");
    assert_eq!(records.len(), 14);
    for (slot, fields) in &records {
        // Every hook phase from request.before through request.after observes the same two records.
        assert_eq!(fields["count"], 2, "{slot:?}");
    }
    let root_after = records
        .iter()
        .find(|(slot, fields)| *slot == FieldAfter && fields["field"]["path"] == json!(["held"]))
        .unwrap();
    assert_eq!(
        root_after.1["result"],
        json!({"value":[{"__typename":"User","id":"U","name":"original"},{"__typename":"User","id":"User-2","name":"phase"}],"errors":[]})
    );
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap()
            .size()
            .0,
        5
    );
    fixture.stop().await;
}

async fn create(fixture: &Fixture, name: &str) {
    let query = format!("mutation {{ createUser(input:{{name:\"{name}\"}}) {{name}} }}");
    assert_eq!(
        fixture.query(&query).await,
        (json!({"data":{"createUser":{"name":name}}}), None)
    );
}

#[tokio::test]
async fn hk_11_12_competing_mutation_waits_through_before_commit_or_rollback_and_after() {
    let _case = crate::vm_gate::test_case();
    for failure in [false, true] {
        let before = Pause::new("held", Stage::BeforeResolver);
        let after = Pause::new("held", Stage::AfterResolver);
        let incoming = Pause::new("Competing", Stage::BeforeOperation);
        let fixture = Fixture::new(BLOG, SEED, &[("Mutation.createUser",
            "local id='User-' .. (ctx.models.User:count({}) + 1) local user=ctx.models.User:create({id=id,name=args.input.name}) if args.input.name=='rollback' then error('fixture failure') end return user")], HOOKS)
            .scheduled(Schedule::new(&[before.clone(), after.clone(), incoming.clone()]));
        let query = format!(
            "mutation {{ held:createUser(input:{{name:\"{}\"}}) {{id name}} }}",
            if failure { "rollback" } else { "committed" }
        );
        let (held, (), competing) = tokio::join!(
            fixture.query(&query),
            async {
                before.reached().await;
                incoming.reached().await;
                incoming.proceed().await;
                assert_queued(&fixture).await;
                before.proceed().await;
                after.reached().await;
                // Returning from the resolver does not admit the next mutation until `field.after` completes.
                assert_queued(&fixture).await;
                let response = fixture.query("{ getUsers {name} }").await;
                // The candidate remains unpublished through `field.after`, so the query sees the original generation.
                let names = json!([{"name":"original"}]);
                assert_eq!(response, (json!({"data":{"getUsers":names}}), None));
                after.proceed().await;
            },
            fixture.query(
                r#"mutation Competing { competing:createUser(input:{name:"next"}) {id name} }"#
            )
        );
        let committed = if failure { 1 } else { 2 };
        if failure {
            assert_eq!(held.1, None);
            assert_eq!(held.0["data"], json!(null));
            assert_eq!(
                held.0["errors"][0]["extensions"]["code"],
                "MOCK_EXECUTION_FAILED"
            );
        } else {
            assert_eq!(
                held,
                (
                    json!({"data":{"held":{"id":"User-2","name":"committed"}}}),
                    None
                )
            );
        }
        assert_eq!(
            competing,
            (
                json!({"data":{"competing":{"id":format!("User-{}", committed + 1),"name":"next"}}}),
                None
            )
        );
        let held_records = records(&fixture, "held");
        let before = held_records
            .iter()
            .find(|(slot, fields)| {
                *slot == FieldBefore && fields["field"]["path"] == json!(["held"])
            })
            .unwrap();
        let after = held_records.iter().find(|(slot, fields)| {
            *slot == FieldAfter && fields["field"]["path"] == json!(["held"])
        });
        assert_eq!(before.1["count"], 1);
        if failure {
            assert!(after.is_none());
        } else {
            let after = after.unwrap();
            assert_eq!(after.1["count"], committed);
            assert!(after.1["result"]["errors"].as_array().unwrap().is_empty());
        }
        let competing_records = records(&fixture, "competing");
        let next = competing_records
            .iter()
            .find(|(slot, fields)| {
                *slot == FieldBefore && fields["field"]["path"] == json!(["competing"])
            })
            .unwrap();
        assert_eq!(next.1["count"], committed);
        assert_eq!(
            fixture
                .worker
                .client
                .snapshot(Instant::now() + Duration::from_secs(1))
                .await
                .unwrap()
                .size()
                .0,
            committed as usize + 1
        );
        fixture.stop().await;
    }
}

async fn assert_queued(fixture: &Fixture) {
    // Complete an unrelated snapshot job so the worker keeps polling an admitted competing request.
    // Then verify for a bounded interval that its `field.before` has not started.
    fixture
        .worker
        .client
        .snapshot(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!fixture.events.lock().unwrap().iter().any(|event| {
        event.slot == FieldBefore
            && event.path.as_ref() == [chigiri::PathSegment::from("competing")]
    }));
}

fn records(fixture: &Fixture, alias: &str) -> Vec<(HookSlot, serde_json::Value)> {
    let events = fixture.events.lock().unwrap();
    let id = events
        .iter()
        .find(|event| {
            event.slot == FieldBefore && event.path.as_ref() == [chigiri::PathSegment::from(alias)]
        })
        .unwrap()
        .request_id;
    events
        .iter()
        .filter(|event| event.request_id == id)
        .map(|event| {
            let fields = &event
                .record
                .as_ref()
                .expect("hook record without diagnostic")
                .fields;
            (
                event.slot,
                chigiri_http::value_to_json(&chigiri::Value::Object(fields.clone())).unwrap(),
            )
        })
        .collect()
}
