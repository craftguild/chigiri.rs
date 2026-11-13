//! A caught pure VM error still invalidates the request candidate and cannot expose partial state.

use super::*;

#[tokio::test]
async fn pure_vm_errors_caught_by_pcall_still_roll_back_the_candidate() {
    let _case = crate::vm_gate::test_case();
    for failure in [
        "pcall(function() error('private-atomic-error') end) return 2",
        "local f:any=nil pcall(f) return 2",
        "local call:any=pcall call() return 2",
    ] {
        let fixture = fixture_failure(&[], failure);
        assert_failed(&request(&fixture, "mutation {first:add(id:\"a\"){id} fail}").await);
        let response = request(&fixture, "{users{id}}").await;
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value, json!({"data":{"users":[]}}));
        let response = request(&fixture, "mutation{add(id:\"a\"){id}}").await;
        assert_eq!(response.status, 200);
        fixture.stop().await;
    }
}
