//! Run captured timed Records through the actual fresh GraphQL event pipeline.

use chigiri::__streaming::SubscriptionOutput;
use serde_json::json;

use super::support::{Fixture, next, output, success};

#[tokio::test]
async fn repeating_sequence_keeps_captured_record_fields_and_refreshes_nested_reads() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::with_subscription_body(
        "return ctx.generator.sequence.Item({values=ctx.models.Item:find({}),interval_ms=20,loop=true})",
    );
    success(
        &fixture
            .execute("mutation { publish(id:\"timed\") { id } }")
            .await,
    );
    let mut subscription = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { edit(id:\"timed\",value:99) { id } }")
            .await,
    );
    assert_eq!(
        next(&mut subscription.live).await,
        json!({"chosen":{"id":"timed","value":2,"current":99}})
    );
    success(
        &fixture
            .execute("mutation { edit(id:\"timed\",value:100) { id } }")
            .await,
    );
    assert_eq!(
        next(&mut subscription.live).await,
        json!({"chosen":{"id":"timed","value":2,"current":100}})
    );
    subscription.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn finite_sequence_completes_after_its_accepted_record_and_next_source_starts_fresh() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::with_subscription_body(
        "return ctx.generator.sequence.Item({values=ctx.models.Item:find({}),interval_ms=20})",
    );
    success(
        &fixture
            .execute("mutation { publish(id:\"finite\") { id } }")
            .await,
    );
    let mut first = fixture.subscribe().await;
    assert_eq!(
        next(&mut first.live).await,
        json!({"chosen":{"id":"finite","value":2,"current":2}})
    );
    assert!(matches!(
        output(&mut first.live).await,
        SubscriptionOutput::Complete
    ));
    first.close().await;
    success(
        &fixture
            .execute("mutation { edit(id:\"finite\",value:7) { id } }")
            .await,
    );
    let mut fresh = fixture.subscribe().await;
    assert_eq!(
        next(&mut fresh.live).await,
        json!({"chosen":{"id":"finite","value":7,"current":7}})
    );
    assert!(matches!(
        output(&mut fresh.live).await,
        SubscriptionOutput::Complete
    ));
    fresh.close().await;
    fixture.stop().await;
}
