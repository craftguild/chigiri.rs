//! Resolver business failures are explicit HTTP-200 errors and request-atomic.

use super::*;

#[tokio::test]
async fn sa_11_business_failure_survives_pcall_exposes_only_its_payload_and_rolls_back() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture_failure(
        &[],
        "pcall(function() ctx.fail({code='INSUFFICIENT_BALANCE',message='Balance too low'}) end) return 2",
    );
    let response = request(
        &fixture,
        "mutation { add(id:\"discard\") { id } denied:fail later:count }",
    )
    .await;
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["data"], json!(null));
    assert_eq!(value["errors"][0]["message"], "Balance too low");
    assert_eq!(
        value["errors"][0]["extensions"]["code"],
        "INSUFFICIENT_BALANCE"
    );
    assert!(value["errors"][0]["extensions"]["requestId"].is_string());
    assert_eq!(value["errors"][0]["path"], json!(["denied"]));
    super::rejection::empty_and_recover(&fixture).await;
    fixture.stop().await;
}

#[tokio::test]
async fn sa_11_invalid_payloads_and_independent_errors_remain_internal() {
    let _case = crate::vm_gate::test_case();
    for failure in [
        "ctx.fail({code='MOCK_RESERVED'}) return 2",
        "ctx.fail({code='lowercase'}) return 2",
        "ctx.fail({code='DENIED',extra=true}) return 2",
        "pcall(function() ctx.fail({code='DENIED'}) end) error('private')",
    ] {
        let fixture = fixture_failure(&[], failure);
        let response = request(&fixture, "mutation { add(id:\"discard\") { id } fail }").await;
        assert_eq!(
            response.status,
            500,
            "{failure}: {}",
            String::from_utf8_lossy(&response.body)
        );
        assert!(!String::from_utf8_lossy(&response.body).contains("private"));
        super::rejection::empty_and_recover(&fixture).await;
        fixture.stop().await;
    }

    let fixture = fixture_failure(
        &[],
        "pcall(function() ctx.fail({code='DENIED'}) end) while true do end return 2",
    );
    let response = request(&fixture, "mutation { add(id:\"discard\") { id } fail }").await;
    assert_eq!(response.status, 500);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_ne!(body["errors"][0]["extensions"]["code"], "DENIED");
    assert!(matches!(
        body["errors"][0]["extensions"]["code"].as_str(),
        Some("MOCK_EXECUTION_TIMEOUT" | "MOCK_EXECUTION_LIMIT_EXCEEDED")
    ));
    super::rejection::empty_and_recover(&fixture).await;
    fixture.stop().await;
}
