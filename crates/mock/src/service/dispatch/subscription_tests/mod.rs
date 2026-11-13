//! Component consumers of the real owned Mock source and fresh event pipeline.

mod failures;
mod lifecycle;
mod positive_drop_logging;
mod pressure;
mod sequences;
mod source_drop_without_logging;
mod support;

use serde_json::json;

use support::{Fixture, next, success};

#[tokio::test]
async fn captured_roots_keep_commit_order_while_nested_reads_refresh_for_each_event() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe().await;
    let response = fixture
        .execute("mutation { publish(id:\"item\") { id value } }")
        .await;
    success(&response);
    let response = fixture
        .execute("mutation { edit(id:\"item\", value:99) { id } }")
        .await;
    success(&response);
    let first = next(&mut subscription.live).await;
    assert_eq!(
        first,
        json!({"chosen":{"id":"item","value":1,"current":99}})
    );
    let response = fixture
        .execute("mutation { edit(id:\"item\", value:100) { id } }")
        .await;
    success(&response);
    let second = next(&mut subscription.live).await;
    assert_eq!(
        second,
        json!({"chosen":{"id":"item","value":2,"current":100}})
    );
    subscription.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn later_root_failure_discards_events_before_a_later_request_recovers() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe().await;
    let response = fixture
        .execute("mutation { publish(id:\"discarded\") { id } fail }")
        .await;
    assert!(!response["errors"].as_array().unwrap().is_empty());
    let response = fixture
        .execute("mutation { publish(id:\"kept\") { id } }")
        .await;
    success(&response);
    for value in [1, 2] {
        let event = next(&mut subscription.live).await;
        assert_eq!(
            event,
            json!({"chosen":{"id":"kept","value":value,"current":2}})
        );
    }
    success(&fixture.execute("{ ping }").await);
    subscription.close().await;
    fixture.stop().await;
}
