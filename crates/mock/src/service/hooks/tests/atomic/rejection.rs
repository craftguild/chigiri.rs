//! Verify independent failures and invalid statuses after rejection never return partial success or empty errors.

use super::*;

pub(super) async fn empty_and_recover(fixture: &Fixture) {
    let response = request(fixture, "{ users { id } }").await;
    assert_eq!(response.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["data"]["users"],
        json!([])
    );
    assert_eq!(
        request(fixture, "mutation { add(id:\"recovered\") { id } }")
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn independent_error_or_deadline_overrides_caught_200_rejection_and_rolls_back() {
    let _case = crate::vm_gate::test_case();
    for (tail, code) in [
        ("error('private-atomic-error')", "MOCK_EXECUTION_FAILED"),
        ("while true do end", "MOCK_EXECUTION_TIMEOUT"),
        (
            "local level: any = 'unknown' ctx.log(level,'private-atomic-error')",
            "MOCK_EXECUTION_FAILED",
        ),
    ] {
        let body = format!(
            "if ctx.field.coordinate=='Mutation.fail' then pcall(function() ctx.reject({{code=200}}) end) {tail} end return nil"
        );
        let fixture = fixture(&[(HookSlot::FieldBefore, &body)]);
        let response = request(
            &fixture,
            "mutation { add(id:\"discard\") { id } fail later:count }",
        )
        .await;
        assert_eq!(response.status, 500);
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["data"], json!(null));
        assert_eq!(value["errors"][0]["extensions"]["code"], code);
        assert!(!String::from_utf8_lossy(&response.body).contains("private-atomic-error"));
        empty_and_recover(&fixture).await;
    }
}

#[tokio::test]
async fn valid_rejection_statuses_produce_graphql_errors_and_invalid_statuses_fail() {
    let _case = crate::vm_gate::test_case();
    for (status, expected, code) in [
        (200, 200, "MOCK_REJECTED"),
        (401, 401, "MOCK_REJECTED"),
        (503, 503, "MOCK_REJECTED"),
        (204, 500, "MOCK_EXECUTION_FAILED"),
        (302, 500, "MOCK_EXECUTION_FAILED"),
        (499, 500, "MOCK_EXECUTION_FAILED"),
    ] {
        let body = format!(
            "if ctx.field.coordinate=='Mutation.fail' then ctx.reject({{code={status},message='rejected'}}) end return nil"
        );
        let fixture = fixture(&[(HookSlot::FieldBefore, &body)]);
        let response = request(
            &fixture,
            "mutation { add(id:\"discard\") { id } fail later:count }",
        )
        .await;
        assert_eq!(response.status, expected, "status {status}");
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["data"], json!(null));
        assert_eq!(value["errors"][0]["extensions"]["code"], code);
        assert!(value["errors"][0]["extensions"]["requestId"].is_string());
        empty_and_recover(&fixture).await;
    }
}
