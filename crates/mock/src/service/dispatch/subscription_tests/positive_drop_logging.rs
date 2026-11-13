//! Compose genuine Mock lifecycle logging with the official bounded HTTP supervisor.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use chigiri::__streaming::{
    AcceptedHttpStreamingFrame, HttpStreamingBody, HttpStreamingDriver, PreparedHttpSubscription,
};
use serde_json::Value;

use super::support::{Fixture, success};

const QUERY: &str = "subscription { chosen: changed(topic:\"events\") { id value } }";
const QUEUED_EVENTS: usize = 32;
const MUTATIONS: usize = 17;
const PUBLICATIONS: usize = MUTATIONS * 2;
const DROPPED: u64 = 2;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn genuine_drop_oldest_logs_match_survivor_and_frozen_terminal() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::with_subscription_body(
        "return ctx.pubsub.subscribe.Item({topic=args.topic,overflow='drop_oldest'})",
    );
    assert_eq!(
        fixture
            .endpoint
            .configuration()
            .subscription_limits()
            .effective()
            .max_queued_events,
        QUEUED_EVENTS
    );
    let (prepared, guard) = fixture.prepare_monitored(QUERY).await;
    let PreparedHttpSubscription::Ready(ready) = prepared else {
        panic!("genuine Mock source did not prepare")
    };
    let (registration, driver) = fixture.endpoint.__http_streaming_supervision().unwrap();
    let response = registration.attach(ready).unwrap();
    let mut driver = Box::pin(driver);
    let mut body = response.into_body();
    let mut records = fixture.take_subscription_logs();
    let generation = registered_generation(&records);

    for index in 0..MUTATIONS {
        success(
            &fixture
                .execute_logged(&format!(
                    "mutation {{ publish(id:\"drop-{index:02}\") {{ id }} }}"
                ))
                .await,
        );
    }
    records.extend(fixture.take_subscription_logs());
    assert_eq!(records.len(), 1 + PUBLICATIONS, "{records:?}");
    assert_publications(&records);
    assert!(
        records
            .iter()
            .all(|record| record["kind"] != "subscription_drop")
    );

    drive_until_drops(&fixture, &mut driver, &mut records).await;
    assert_drop_records(&records, generation);

    let initial = next_frame(&mut driver, &mut body).await;
    assert!(initial.as_ref().is_empty());
    drop(initial);
    let survivor = next_frame(&mut driver, &mut body).await;
    let wire = std::str::from_utf8(survivor.as_ref()).unwrap();
    assert!(wire.contains("\"id\":\"drop-01\""), "{wire}");
    assert!(wire.contains("\"value\":1"), "{wire}");
    assert!(wire.contains("\"droppedTotal\":2"), "{wire}");

    registration.close();
    drive_until_terminal(&fixture, &mut driver, &mut records).await;
    assert_terminal(&records, generation);
    assert_private(&records);
    assert!(fixture.take_subscription_logs().is_empty());

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

fn registered_generation(records: &[Value]) -> u64 {
    assert_eq!(records.len(), 1, "{records:?}");
    let setup = &records[0];
    assert_eq!(setup["format"], "chigiri-mock-log/v5");
    assert_eq!(setup["kind"], "subscription_setup");
    assert_eq!(setup["coordinate"], "Signals.changed");
    assert_eq!(setup["overflow"], "drop_oldest");
    assert_eq!(setup["outcome"], "registered");
    assert!(setup["request_id"].as_u64().is_some());
    setup["subscription_id"].as_u64().unwrap()
}

fn assert_publications(records: &[Value]) {
    let publications: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "subscription_publication")
        .collect();
    assert_eq!(publications.len(), PUBLICATIONS);
    for (offset, record) in publications.into_iter().enumerate() {
        assert_eq!(record["format"], "chigiri-mock-log/v5");
        // Registration owns sequence 1; committed publication batches are 2 through 18.
        assert_eq!(record["sequence"], u64::try_from(offset / 2 + 2).unwrap());
        assert_eq!(record["index"], u64::try_from(offset % 2).unwrap());
        assert_eq!(record["coordinate"], "Signals.changed");
        assert_eq!(record["outcome"], "staged");
        assert!(record["request_id"].as_u64().is_some());
    }
}

fn assert_drop_records(records: &[Value], generation: u64) {
    assert_eq!(records.len(), 1 + PUBLICATIONS + DROPPED as usize);
    assert!(
        records[1..=PUBLICATIONS]
            .iter()
            .all(|record| record["kind"] == "subscription_publication")
    );
    let drops: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "subscription_drop")
        .collect();
    assert_eq!(drops.len(), DROPPED as usize);
    for (offset, record) in drops.into_iter().enumerate() {
        assert_eq!(&records[1 + PUBLICATIONS + offset], record);
        assert_eq!(record["request_id"], Value::Null);
        assert_eq!(record["subscription_id"], generation);
        assert_eq!(record["coordinate"], "Signals.changed");
        assert_eq!(record["reason"], "drop_oldest");
        assert_eq!(record["dropped_total"], (offset + 1) as u64);
    }
}

fn assert_terminal(records: &[Value], generation: u64) {
    assert_eq!(records.len(), 2 + PUBLICATIONS + DROPPED as usize);
    let terminals: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "subscription_terminal")
        .collect();
    assert_eq!(terminals.len(), 1);
    let terminal = terminals[0];
    assert_eq!(terminal["request_id"], Value::Null);
    assert_eq!(terminal["subscription_id"], generation);
    assert_eq!(terminal["coordinate"], "Signals.changed");
    assert_eq!(terminal["outcome"], "cancelled");
    assert_eq!(terminal["cause"], "server_shutdown");
    assert_eq!(terminal["dropped_total"], DROPPED);
    assert_eq!(records.last(), Some(terminal));
}

fn assert_private(records: &[Value]) {
    let text = serde_json::to_string(records).unwrap();
    for private in ["events", "drop-", "subscriber"] {
        assert!(!text.contains(private), "private value in logs: {text}");
    }
}

async fn drive_until_drops(
    fixture: &Fixture,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    records: &mut Vec<Value>,
) {
    tokio::time::timeout(
        Duration::from_secs(8),
        poll_fn(|context| {
            assert!(driver.as_mut().poll(context).is_pending());
            records.extend(fixture.take_subscription_logs());
            let drops = records
                .iter()
                .filter(|record| record["kind"] == "subscription_drop")
                .count();
            if drops == DROPPED as usize {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .unwrap();
}

async fn drive_until_terminal(
    fixture: &Fixture,
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    records: &mut Vec<Value>,
) {
    tokio::time::timeout(
        Duration::from_secs(8),
        poll_fn(|context| {
            assert!(driver.as_mut().poll(context).is_pending());
            records.extend(fixture.take_subscription_logs());
            if records
                .iter()
                .any(|record| record["kind"] == "subscription_terminal")
            {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .unwrap();
}

pub(super) async fn next_frame(
    driver: &mut Pin<Box<HttpStreamingDriver>>,
    body: &mut HttpStreamingBody,
) -> AcceptedHttpStreamingFrame {
    tokio::time::timeout(
        Duration::from_secs(8),
        poll_fn(|context| {
            assert!(driver.as_mut().poll(context).is_pending());
            match body.poll_next(context) {
                Poll::Ready(Some(Ok(frame))) => Poll::Ready(frame.accept().unwrap()),
                Poll::Ready(Some(Err(error))) => panic!("body failed: {error}"),
                Poll::Ready(None) => panic!("body ended before the expected frame"),
                Poll::Pending => Poll::Pending,
            }
        }),
    )
    .await
    .unwrap()
}
