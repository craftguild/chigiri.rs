//! Exercise hard projection failure and policy rejection through real source ownership.

use chigiri::__streaming::SubscriptionOutput;

use super::support::{Fixture, next, output, success};
use crate::hooks::HookSlot;

#[tokio::test]
async fn exhausted_projection_terminates_but_a_fresh_source_uses_the_same_worker() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"exhausted\") { id } }")
            .await,
    );
    let failed =
        crate::service::hooks::exhaust_projection_for_test(output(&mut subscription.live)).await;
    let SubscriptionOutput::Terminal(response) = failed else {
        panic!("exhausted projection did not terminate immediately")
    };
    assert!(response.data.is_none());
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&chigiri::Value::from("MOCK_EXECUTION_LIMIT_EXCEEDED"))
    );
    subscription.close().await;
    success(&fixture.execute("{ ping }").await);
    let mut later = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"fresh\") { id } }")
            .await,
    );
    assert_eq!(next(&mut later.live).await["chosen"]["id"], "fresh");
    later.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn field_policy_rejection_is_an_all_null_event_without_source_failure() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::with_hooks(&[(
        HookSlot::FieldBefore,
        "local parent:any=ctx.field.parent if ctx.field.coordinate=='Item.current' and type(parent)=='table' and parent.value==1 then ctx.reject({code=403,message='event policy denied'}) end",
    )]);
    let mut subscription = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"policy\") { id } }")
            .await,
    );
    let SubscriptionOutput::Event(response) = output(&mut subscription.live).await else {
        panic!("policy rejection terminated the source")
    };
    assert_eq!(response.data, Some(chigiri::Value::Null));
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&chigiri::Value::from("MOCK_REJECTED"))
    );
    assert_eq!(next(&mut subscription.live).await["chosen"]["value"], 2);
    subscription.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn per_vm_host_call_limit_is_terminal_and_the_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture
        .subscribe_query(
            "subscription { chosen: changed(topic:\"events\") { current(mode:\"local\") } }",
        )
        .await;
    success(
        &fixture
            .execute("mutation { publish(id:\"local\") { id } }")
            .await,
    );
    let SubscriptionOutput::Terminal(response) = output(&mut subscription.live).await else {
        panic!("local VM hard limit did not terminate")
    };
    assert!(response.data.is_none());
    assert_eq!(
        response.errors[0].extensions.get("limit"),
        Some(&chigiri::Value::from("hostCalls"))
    );
    subscription.close().await;
    success(&fixture.execute("{ ping }").await);
    let mut later = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"later\") { id } }")
            .await,
    );
    assert_eq!(next(&mut later.live).await["chosen"]["id"], "later");
    later.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn output_completion_limit_is_terminal_and_the_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = Fixture::new();
    let healthy = fixture.endpoint.clone();
    fixture.endpoint =
        crate::service::dispatch::limited_nodes_for_test(fixture.worker.client.clone(), 1);
    let mut subscription = fixture.subscribe().await;
    fixture.endpoint = healthy;
    success(
        &fixture
            .execute("mutation { publish(id:\"output\") { id } }")
            .await,
    );
    let SubscriptionOutput::Terminal(response) = output(&mut subscription.live).await else {
        panic!("output limit did not terminate")
    };
    assert!(response.data.is_none());
    assert!(!response.errors.is_empty());
    subscription.close().await;
    success(&fixture.execute("{ ping }").await);
    let mut later = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"after-output\") { id } }")
            .await,
    );
    assert_eq!(next(&mut later.live).await["chosen"]["id"], "after-output");
    later.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn vm_deadline_is_terminal_and_a_later_source_uses_the_same_worker() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture
        .subscribe_query(
            "subscription { chosen: changed(topic:\"events\") { current(mode:\"timeout\") } }",
        )
        .await;
    success(
        &fixture
            .execute("mutation { publish(id:\"timeout\") { id } }")
            .await,
    );
    let SubscriptionOutput::Terminal(response) = output(&mut subscription.live).await else {
        panic!("VM deadline did not terminate")
    };
    assert!(response.data.is_none());
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&chigiri::Value::from("MOCK_EXECUTION_TIMEOUT"))
    );
    subscription.close().await;
    success(&fixture.execute("{ ping }").await);
    let mut later = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"after-timeout\") { id } }")
            .await,
    );
    assert_eq!(next(&mut later.live).await["chosen"]["id"], "after-timeout");
    later.close().await;
    fixture.stop().await;
}
