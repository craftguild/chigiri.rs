//! Verify read-only after-hook work limits roll back the current root without reverting an earlier commit.

use super::*;
use serde_json::json;

#[tokio::test]
async fn after_hook_work_failure_rolls_back_the_current_root_and_the_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    // Type testing and fixtures use the same native stack as existing larger fixtures.
    let fixture = std::thread::Builder::new().stack_size(8 * 1024 * 1024).spawn(|| Fixture::new(
        "type A{text:String!} type B{name:String!} type Query{counts:[Int!]!} type Mutation{a(text:String!):Int! b:Int!}",
        &json!({"A":[{"text":"x".repeat(4096)}]}).to_string(),
        &[
            (
                "Mutation.a",
                "ctx.models.A:create({text=args.text}) return 1",
            ),
            (
                "Mutation.b",
                "ctx.models.B:create({name='candidate'}) return 2",
            ),
            (
                "Query.counts",
                "return {ctx.models.A:count({}),ctx.models.B:count({})}",
            ),
        ],
        &[(
            HookSlot::FieldAfter,
            r#"
            if ctx.field.path[1] == 'second' then
                ctx.log('info', 'after-work-entered')
                pcall(function()
                    for i=1,130 do local rows=ctx.models.A:find({}) end
                end)
            end
            return nil
        "#,
        )],
    )).unwrap().join().unwrap();
    let body = serde_json::to_vec(&json!({
        "query":"mutation($text:String!){first:a(text:$text) second:b}",
        "variables":{"text":"x".repeat(4096)}
    }))
    .unwrap();
    let (http, _) = fixture.request("POST", None, body, &[]).await;
    assert_eq!(http.status, 500);
    let response: serde_json::Value = serde_json::from_slice(&http.body).unwrap();
    assert_eq!(response["data"], json!(null));
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(response["errors"][0]["extensions"]["limit"], "work");
    assert!(fixture.events.lock().unwrap().iter().any(|event| {
        event.slot == HookSlot::FieldAfter
            && event
                .record
                .as_ref()
                .is_ok_and(|record| record.message == "after-work-entered")
    }));
    assert_eq!(
        fixture.query("{counts}").await.0,
        json!({"data":{"counts":[1,0]}})
    );
    assert_eq!(
        fixture.query("mutation{a(text:\"next\") b}").await.0,
        json!({"data":{"a":1,"b":2}})
    );
    assert_eq!(
        fixture.query("{counts}").await.0,
        json!({"data":{"counts":[2,1]}})
    );
    fixture.stop().await;
}
