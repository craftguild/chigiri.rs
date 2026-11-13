//! Actual source loss remains visible on the wire with no installed log sink.

use std::time::Duration;

use chigiri::__streaming::PreparedHttpSubscription;

use super::positive_drop_logging::next_frame;
use super::support::{Fixture, success};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_drop_metadata_does_not_require_subscription_logging() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::without_subscription_logging(
        "return ctx.pubsub.subscribe.Item({topic=args.topic,overflow='drop_oldest'})",
    );
    let (prepared, guard) = fixture
        .prepare_monitored("subscription { changed(topic:\"events\") { id value } }")
        .await;
    let PreparedHttpSubscription::Ready(ready) = prepared else {
        panic!("source setup failed")
    };
    let (registration, driver) = fixture.endpoint.__http_streaming_supervision().unwrap();
    let mut body = registration.attach(ready).unwrap().into_body();
    let mut driver = Box::pin(driver);
    for index in 0..17 {
        success(
            &fixture
                .execute(&format!(
                    "mutation {{ publish(id:\"silent-{index:02}\") {{ id }} }}"
                ))
                .await,
        );
    }
    assert!(fixture.take_subscription_logs().is_empty());
    // Wait for actual host fan-out, then start SDK demand. A successful Mutation
    // return precedes the asynchronous pump's final cursor transition.
    tokio::time::timeout(Duration::from_secs(8), async {
        while !fixture.worker.client.publication_stage_idle_for_test() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let initial = next_frame(&mut driver, &mut body).await;
    assert!(initial.as_ref().is_empty());
    drop(initial);
    let survivor = next_frame(&mut driver, &mut body).await;
    let wire = std::str::from_utf8(survivor.as_ref()).unwrap();
    assert!(wire.contains("\"id\":\"silent-01\""), "{wire}");
    assert!(wire.contains("\"droppedTotal\":2"), "{wire}");
    assert!(fixture.take_subscription_logs().is_empty());
    registration.close();
    drop(survivor);
    tokio::time::timeout(Duration::from_secs(8), driver.as_mut())
        .await
        .unwrap()
        .unwrap();
    drop(body);
    guard.shutdown().unwrap().await.unwrap();
    assert!(fixture.take_subscription_logs().is_empty());
    fixture.stop().await;
}
