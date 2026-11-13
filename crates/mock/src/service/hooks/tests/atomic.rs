//! Keep HTTP, root execution, and after-hook changes in one candidate until encoding succeeds.

use super::*;
use serde_json::json;

mod business;
mod output;
mod pcall;
mod rejection;

const SDL: &str = "type Query { users: [User!]! } type Mutation { add(id: ID!): User! fail: Int count: Int! } type User { id: ID! name: String! }";

fn fixture(hooks: &[(HookSlot, &str)]) -> Fixture {
    fixture_failure(hooks, "error('private-atomic-error')")
}

fn fixture_failure(hooks: &[(HookSlot, &str)], failure: &str) -> Fixture {
    Fixture::new(
        SDL,
        "{}",
        &[
            ("Query.users", "return ctx.models.User:find({})"),
            (
                "Mutation.add",
                "return ctx.models.User:create({id=args.id,name=args.id})",
            ),
            ("Mutation.fail", failure),
            ("Mutation.count", "return ctx.models.User:count({})"),
        ],
        hooks,
    )
}

async fn request(fixture: &Fixture, query: &str) -> ResponseParts {
    fixture
        .request(
            "POST",
            None,
            serde_json::to_vec(&json!({"query":query})).unwrap(),
            &[],
        )
        .await
        .0
}

fn assert_failed(response: &ResponseParts) {
    assert_failure_code(response, "MOCK_EXECUTION_FAILED");
}

fn assert_failure_code(response: &ResponseParts, code: &str) {
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["extensions"]["code"], code);
    assert!(value["errors"][0]["extensions"]["requestId"].is_string());
    assert!(!String::from_utf8_lossy(&response.body).contains("private-atomic-error"));
}

#[tokio::test]
async fn hooks_read_one_candidate_and_the_request_publishes_its_final_state() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[
        (
            HookSlot::FieldAfter,
            "ctx.log('info','candidate',{count=ctx.models.User:count({})})",
        ),
        (
            HookSlot::OperationAfter,
            "ctx.log('info','candidate',{count=ctx.models.User:count({})})",
        ),
        (
            HookSlot::RequestAfter,
            "ctx.log('info','candidate',{count=ctx.models.User:count({})})",
        ),
    ]);
    let response = request(
        &fixture,
        "mutation { first:add(id:\"a\") { id } second:add(id:\"b\") { id } count }",
    )
    .await;
    assert_eq!(response.status, 200);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(
        value["data"],
        json!({"first":{"id":"a"},"second":{"id":"b"},"count":2})
    );
    let logs = fixture.logs();
    assert!(
        logs.iter()
            .any(|(slot, value)| *slot == HookSlot::FieldAfter && value["count"] == 1)
    );
    for slot in [HookSlot::OperationAfter, HookSlot::RequestAfter] {
        assert!(
            logs.iter()
                .any(|(found, value)| *found == slot && value["count"] == 2)
        );
    }
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        2
    );
}

#[tokio::test]
async fn root_failure_rolls_back_prior_changes_and_the_next_request_recovers() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[]);
    let response = request(&fixture, "mutation { add(id:\"discard\") { id } fail }").await;
    assert_failed(&response);
    let response = request(&fixture, "{ users { id } }").await;
    assert_eq!(response.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["data"]["users"],
        json!([])
    );
    let response = request(&fixture, "mutation { add(id:\"next\") { id } }").await;
    assert_eq!(response.status, 200);
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        1
    );
}

#[tokio::test]
async fn request_after_failure_rolls_back_all_prior_root_changes() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[(HookSlot::RequestAfter, "error('private-atomic-error')")]);
    let response = request(
        &fixture,
        "mutation { first:add(id:\"a\") { id } second:add(id:\"b\") { id } }",
    )
    .await;
    assert_failed(&response);
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
}

#[tokio::test]
async fn pre_execution_document_failure_preserves_the_existing_400_response() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[(HookSlot::RequestBefore, "error('must-not-run')")]);
    let response = request(&fixture, "{").await;
    assert_eq!(response.status, 400);
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn direct_http_handling_keeps_scope_hook_and_candidate_under_one_owner() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[(
        HookSlot::RequestAfter,
        "ctx.log('info','result',{result=ctx.result})",
    )]);
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let body =
        serde_json::to_vec(&json!({"query":"mutation { add(id:\"direct\") { id } }"})).unwrap();
    let response = fixture
        .endpoint
        .handle(transport, BufferedBody::new(body), &Clock)
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        1
    );
}

#[tokio::test]
async fn encode_limit_failure_discards_a_ready_candidate() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = fixture(&[]);
    fixture.endpoint =
        crate::service::dispatch::limited_for_test(fixture.worker.client.clone(), 128);
    let query = format!(
        "mutation {{ first:add(id:\"{}\") {{ id name }} second:add(id:\"{}\") {{ id name }} }}",
        "a".repeat(80),
        "b".repeat(80)
    );
    let response = request(&fixture, &query).await;
    assert_failure_code(&response, "MOCK_EXECUTION_LIMIT_EXCEEDED");
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
}

#[tokio::test]
async fn child_selection_output_limit_rolls_back_the_root_candidate() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = fixture(&[]);
    fixture.endpoint =
        crate::service::dispatch::limited_nodes_for_test(fixture.worker.client.clone(), 2);
    let response = request(&fixture, "mutation { add(id:\"discard\") { id name } }").await;
    assert_failure_code(&response, "MOCK_EXECUTION_LIMIT_EXCEEDED");
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
}

#[tokio::test]
async fn caught_pcall_failure_discards_the_successful_200_candidate() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[(
        HookSlot::FieldBefore,
        "if ctx.field.path[1]=='stop' then pcall(function() ctx.reject({code=200,message='Denied'}) end) end",
    )]);
    let response = request(
        &fixture,
        "mutation { first:add(id:\"discard\") { id } stop:count }",
    )
    .await;
    assert_eq!(response.status, 200);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["message"], "Denied");
    assert_eq!(value["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
    assert!(value["errors"][0]["extensions"]["requestId"].is_string());
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
}

#[tokio::test]
async fn null_scalar_output_failure_prevents_later_root_hooks_from_starting() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = Fixture::new(
        "scalar Data @representation(type: JSON) type Query { value: Int } type Mutation { bad: Data later: Int }",
        "{}",
        &[
            ("Query.value", "return 1"),
            ("Mutation.bad", "return {a=1,b=2}"),
            ("Mutation.later", "return 7"),
        ],
        &[(
            HookSlot::FieldBefore,
            "ctx.log('info','reached',{field=ctx.field.coordinate})",
        )],
    );
    fixture.endpoint =
        crate::service::dispatch::limited_nodes_for_test(fixture.worker.client.clone(), 2);
    let response = request(&fixture, "mutation { bad later }").await;
    assert_failure_code(&response, "MOCK_EXECUTION_LIMIT_EXCEEDED");
    assert!(!fixture.events.lock().unwrap().iter().any(|event| {
        event.slot == HookSlot::FieldBefore
            && event.path.as_ref() == [chigiri::PathSegment::from("later")]
    }));
}

#[tokio::test]
async fn local_vm_timeout_is_reported_as_root_timeout() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture_failure(&[], "while true do end return 0");
    let started = Instant::now();
    let response = request(&fixture, "mutation { add(id:\"discard\") { id } fail }").await;
    assert_eq!(response.status, 500);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], serde_json::Value::Null);
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_TIMEOUT"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    let next = request(&fixture, "mutation { add(id:\"recovered\") { id } }").await;
    assert_eq!(next.status, 200);
}
