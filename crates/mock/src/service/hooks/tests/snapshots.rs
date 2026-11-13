//! Test parent stored values, pre-selection field results, and committed mutation generations.

use super::*;
use HookSlot::*;
use serde_json::json;

#[tokio::test]
async fn hk_09_11_all_business_fields_observe_stored_parent_without_running_children() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        include_str!("../../../../fixtures/blog.graphql"),
        r#"{"User":[{"id":"U","name":"Ada"}],"Post":[{"id":"P","title":"First","body":"Body","authorId":"U"}]}"#,
        &[],
        &[
            (FieldBefore, "ctx.log('info','before',{field=ctx.field})"),
            (
                FieldAfter,
                "ctx.log('info','after',{field=ctx.field,result=ctx.result})",
            ),
        ],
    );
    let (response,status)=fixture.query("{ rows:getUsers { __typename ...F hidden:name @skip(if:true) } __schema { queryType { name } } } fragment F on User { renamed:name posts { title } }").await;
    assert_eq!(status, None);
    assert_eq!(
        response["data"]["rows"],
        json!([{"__typename":"User","renamed":"Ada","posts":[{"title":"First"}]}])
    );
    let logs = fixture.logs();
    assert_eq!(logs.len(), 8);
    for coordinate in ["Query.getUsers", "User.name", "User.posts", "Post.title"] {
        let entries: Vec<_> = logs
            .iter()
            .filter(|(_, entry)| entry["field"]["coordinate"] == coordinate)
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, FieldBefore);
        assert_eq!(entries[1].0, FieldAfter);
    }
    assert_eq!(logs[0].1["field"]["coordinate"], "Query.getUsers");
    assert_eq!(
        logs[1].1["result"]["value"],
        json!([{"__typename":"User","id":"U","name":"Ada"}])
    );
    let before = logs
        .iter()
        .find(|(slot, log)| *slot == FieldBefore && log["field"]["coordinate"] == "User.name")
        .unwrap();
    assert_eq!(before.1["field"]["path"], json!(["rows", 0, "renamed"]));
    assert_eq!(
        before.1["field"]["parent"],
        json!({"__typename":"User","id":"U","name":"Ada"})
    );
    assert!(logs.iter().all(|(_, entry)| {
        !entry["field"]["coordinate"]
            .as_str()
            .unwrap()
            .contains("__")
    }));
    fixture.stop().await;
}

#[tokio::test]
async fn ra_after_reads_candidate_then_readonly_failure_discards_it_before_later_reject() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        include_str!("../../../../fixtures/blog.graphql"),
        "{}",
        &[],
        &[
            (
                FieldBefore,
                "ctx.log('info','before',{count=ctx.models.User:count({})}) if ctx.field.path[1] == 'denied' then ctx.reject({code=403}) end",
            ),
            (
                FieldAfter,
                "ctx.log('info','after',{count=ctx.models.User:count({})}) if ctx.field.coordinate == 'Mutation.createUser' then local unsafe: any = ctx return unsafe.models.User:create({name='forbidden'}) end return nil",
            ),
            (
                OperationAfter,
                "ctx.log('info','operation',{count=ctx.models.User:count({})})",
            ),
            (
                RequestAfter,
                "ctx.log('info','request',{count=ctx.models.User:count({})})",
            ),
        ],
    );
    let query = "mutation { first:createUser(input:{name:\"Ada\"}) { id } denied:createUser(input:{name:\"No\"}) { id } }";
    let (parts, status) = fixture
        .request(
            "POST",
            None,
            serde_json::to_vec(&json!({"query":query})).unwrap(),
            &[],
        )
        .await;
    assert_eq!(parts.status, 500);
    assert_eq!(status, None);
    let response: serde_json::Value = serde_json::from_slice(&parts.body).unwrap();
    assert_eq!(response["data"], json!(null));
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert!(response["errors"][0]["extensions"]["requestId"].is_string());
    {
        let events = fixture.events.lock().unwrap();
        let records: Vec<_> = events
            .iter()
            .filter_map(|event| {
                event.record.as_ref().ok().map(|record| {
                    (
                        event.slot,
                        event.coordinate.as_deref(),
                        record.fields.get("count").unwrap().clone(),
                    )
                })
            })
            .collect();
        assert_eq!(
            records[0],
            (
                FieldBefore,
                Some("Mutation.createUser"),
                chigiri::Value::Int(0)
            )
        );
        assert_eq!(
            records[1],
            (
                FieldAfter,
                Some("Mutation.createUser"),
                chigiri::Value::Int(1)
            )
        );
        assert!(
            events
                .iter()
                .any(|event| event.slot == FieldAfter && event.record.is_err())
        );
        assert_eq!(records.len(), 2);
        assert!(
            !events
                .iter()
                .any(|event| event.path.first() == Some(&chigiri::PathSegment::from("denied")))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.slot, OperationAfter | RequestAfter))
        );
    }
    fixture.events.lock().unwrap().clear();
    let (response, _) = fixture.query("{ getUsers { id name } }").await;
    assert_eq!(response["data"], json!({"getUsers":[]}));
    assert!(response.get("errors").is_none());
    fixture.stop().await;
}
