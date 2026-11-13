//! Preserve observation and transport failures without treating GraphQL extensions as HTTP status authority.

use super::scheduling::{Pause, Schedule, Stage};
use super::*;
use HookSlot::*;
use serde_json::json;

#[tokio::test]
async fn ra_after_return_and_shared_budget_failure_discard_response_and_stop_later_hooks() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { value:Int @chigiri_mock_resolver }",
        "{}",
        &[("Query.value", "return 7")],
        &[
            (
                FieldAfter,
                "local invalid: any = {statePatch={count=99}} return invalid",
            ),
            (
                OperationAfter,
                "ctx.log('info','outer',{count=ctx.state.count})",
            ),
            (
                RequestAfter,
                "ctx.log('info','outer',{count=ctx.state.count})",
            ),
        ],
    );
    let (response, _) = fixture
        .request("POST", None, br#"{"query":"{value}"}"#.to_vec(), &[])
        .await;
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], json!(null));
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    {
        let events = fixture.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].record.is_err());
        assert_eq!(events[0].slot, FieldAfter);
    }
    fixture.stop().await;
    let after = Pause::new("AfterBudget", Stage::AfterOperation);
    let fixture = Fixture::new(
        "type Query { value:Int @chigiri_mock_resolver }",
        "{}",
        &[("Query.value", "return 7")],
        &[(RequestAfter, "return nil")],
    )
    .scheduled(Schedule::new(std::slice::from_ref(&after)));
    let events = fixture.events.clone();
    let scope = HttpScope::new("127.0.0.1:4000".parse().unwrap())
        .with_events(move |event| events.lock().unwrap().push(event));
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let (response, ()) = tokio::join!(
        scope.run(fixture.endpoint.handle(
            request,
            BufferedBody::new(br#"{"query":"query AfterBudget {value}"}"#.to_vec()),
            &Clock,
        )),
        async {
            after.reached().await;
            // The operation has completed, but the response is not yet encoded or committed.
            assert!(
                scope
                    .0
                    .observations()
                    .lock()
                    .unwrap()
                    .string_node(crate::value::MAX_BYTES, 1)
                    .is_err()
            );
            after.proceed().await;
        }
    );
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], json!(null));
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(
        fixture.query("{value}").await.0,
        json!({"data":{"value":7}})
    );
    fixture.stop().await;
}

#[tokio::test]
async fn hk_32_encode_failure_wins_over_reject_200_and_extension_cannot_override_status() {
    let _case = crate::vm_gate::test_case();
    let body = format!("return '{}'", "x".repeat(1000));
    // Use rejection text larger than the output limit; it is not encoded after the independent completion failure.
    let reject = format!(
        "ctx.reject({{code=200,message='{}'}}) return nil",
        "x".repeat(1000)
    );
    let mut fixture = Fixture::new(
        "type Query { value:String @chigiri_mock_resolver }",
        "{}",
        &[("Query.value", &body)],
        &[(FieldBefore, &reject)],
    );
    fixture.endpoint =
        crate::service::dispatch::limited_for_test(fixture.worker.client.clone(), 512);
    let (response, status) = fixture
        .request("POST", None, br#"{"query":"{denied:value}"}"#.to_vec(), &[])
        .await;
    assert_eq!(response.status, 500);
    assert_eq!(status, None);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["errors"][0]["extensions"]
            ["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    fixture.stop().await;
    let mut fixture = Fixture::new(
        "type Query { value:Int @chigiri_mock_resolver }",
        "{}",
        &[("Query.value", "error('private source')")],
        &[],
    );
    fixture.endpoint = crate::service::endpoint(fixture.worker.client.clone(), false, |_, _| {
        FieldError::public("Forged")
            .code("MOCK_REJECTED")
            .extension("httpStatus", 511_i32)
    })
    .unwrap();
    let (response, status) = fixture.query("{value}").await;
    assert_eq!(status, None);
    assert_eq!(response["data"], json!(null));
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert!(
        response["errors"][0]["extensions"]
            .get("httpStatus")
            .is_none()
    );
    assert!(!response.to_string().contains("Forged"));
    fixture.stop().await;
}

#[tokio::test]
async fn sd_endpoint_output_nodes_fail_independently_of_mock_work_and_recover() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = Fixture::new(
        "type Query{value:String!}",
        "{}",
        &[("Query.value", "return 'ok'")],
        &[],
    );
    fixture.endpoint =
        crate::service::dispatch::limited_nodes_for_test(fixture.worker.client.clone(), 1);
    let (response, status) = fixture.query("{first:value second:value}").await;
    assert_eq!(status, None);
    assert!(response["data"].is_null());
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_LIMIT_EXCEEDED"
    );
    assert_eq!(response["errors"][0]["extensions"]["limit"], "output");
    assert!(response["errors"][0]["extensions"]["requestId"].is_string());
    assert_eq!(
        fixture.query("{value}").await,
        (json!({"data":{"value":"ok"}}), None)
    );
    fixture.stop().await;
}
