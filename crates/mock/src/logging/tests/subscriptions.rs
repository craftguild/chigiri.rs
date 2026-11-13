//! Assert exact publication records and isolated bounded stdout accounting.

use super::*;
use crate::publication::{MAX_PUBLICATIONS, MAX_SAFE_SEQUENCE};

fn publication(sequence: u64, index: usize, coordinate: &str, lost: bool) -> Vec<u8> {
    super::super::publication::record(u64::MAX, sequence, index, coordinate, lost).unwrap()
}

#[test]
fn publication_shapes_preserve_safe_order_ids_and_existing_request_ids() {
    for sequence in [1, MAX_SAFE_SEQUENCE] {
        for index in [0, MAX_PUBLICATIONS - 1] {
            for (lost, outcome) in [(false, "staged"), (true, "lost")] {
                let bytes = publication(sequence, index, "Subscription.changed", lost);
                assert_eq!(bytes.last(), Some(&b'\n'));
                assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert!(value["timestamp"].as_i64().unwrap() > 0);
                value.as_object_mut().unwrap().remove("timestamp");
                assert_eq!(
                    value,
                    json!({
                        "format": "chigiri-mock-log/v5", "request_id": u64::MAX,
                        "kind": "subscription_publication", "sequence": sequence,
                        "index": index, "coordinate": "Subscription.changed", "outcome": outcome
                    })
                );
            }
        }
    }
}

#[test]
fn invalid_identity_or_serialization_counts_only_subscription_log_loss() {
    let (logger, [hooks, accesses, subscriptions, notices]) = queues();
    for (sequence, index) in [
        (0, 0),
        (MAX_SAFE_SEQUENCE + 1, 0),
        (1, MAX_PUBLICATIONS),
        (1, usize::MAX),
    ] {
        logger.publication(1, sequence, index, "Subscription.changed", false);
    }
    let base = publication(1, 0, "", false).len();
    let coordinate = "x".repeat(RECORD_BYTES - base);
    let exact = publication(1, 0, &coordinate, false);
    assert_eq!(exact.len(), RECORD_BYTES);
    // Use the same request ID, so this tests the exact newline-inclusive limit.
    logger.publication(u64::MAX, 1, 0, &coordinate, false);
    assert_eq!(subscriptions.try_recv().unwrap().bytes.len(), exact.len());
    logger.publication(u64::MAX, 1, 0, &(coordinate + "x"), false);
    assert!(hooks.is_empty() && accesses.is_empty() && notices.is_empty());
    assert_eq!(
        logger.loss(),
        Loss {
            subscription: 5,
            ..Loss::default()
        }
    );
}

#[test]
fn subscription_queue_cannot_consume_other_reserved_classes() {
    let (logger, [hooks, accesses, subscriptions, notices]) = queues();
    for _ in 0..QUEUE_RECORDS + 1 {
        logger.publication(1, 1, 0, "Subscription.changed", false);
    }
    logger.hook(event(HookSlot::RequestBefore));
    let mut access = Access::new(logger.clone(), 1, "POST", "/graphql");
    access.response(200);
    access.complete();
    drop(access);
    logger.notice("separate diagnostic");
    assert_eq!(subscriptions.len(), QUEUE_RECORDS);
    assert_eq!((hooks.len(), accesses.len(), notices.len()), (1, 1, 1));
    assert_eq!(
        logger.loss(),
        Loss {
            subscription: 1,
            ..Loss::default()
        }
    );
    drop(subscriptions);
    logger.publication(1, 1, 0, "Subscription.changed", false);
    assert_eq!(logger.loss().subscription, 2);
}

#[test]
fn fair_stdout_rotation_and_shutdown_discard_keep_exact_classes() {
    let (logger, [hooks, accesses, subscriptions, notices]) = queues();
    for kind in [Kind::Hook, Kind::Access, Kind::Subscription] {
        for _ in 0..QUEUE_RECORDS {
            logger.enqueue(kind, Ok(b"record\n".to_vec()));
        }
    }
    for _ in 0..NOTICE_RECORDS {
        logger.notice("notice");
    }
    let stdout = [accesses, hooks, subscriptions];
    let mut cursor = 0;
    for expected in [Kind::Access, Kind::Hook, Kind::Subscription]
        .into_iter()
        .cycle()
        .take(9)
    {
        assert_eq!(
            coordinator::next_stdout(&stdout, &mut cursor).unwrap().kind,
            expected
        );
    }
    logger.stopping.lock().unwrap().close = true;
    coordinator::discard_queues(&stdout, &notices, &logger.counts);
    assert!(stdout.iter().all(crossbeam_channel::Receiver::is_empty) && notices.is_empty());
    assert_eq!(
        logger.loss(),
        Loss {
            hook: 61,
            access: 61,
            subscription: 61,
            notice: 8
        }
    );
    coordinator::discard_queues(&stdout, &notices, &logger.counts);
    logger.publication(1, 1, 0, "Subscription.changed", false);
    assert_eq!(logger.loss().subscription, 62);
}

#[test]
fn loss_schema_saturates_and_broken_sink_counts_original_class_once() {
    let (logger, _) = queues();
    logger
        .counts
        .subscription
        .store(u64::MAX, Ordering::Relaxed);
    logger.counts.lose(Kind::Subscription);
    assert_eq!(logger.loss().subscription, u64::MAX);
    let mut value: serde_json::Value =
        serde_json::from_slice(&records::loss(logger.loss()).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("timestamp");
    assert_eq!(
        value,
        json!({
            "format": "chigiri-mock-log/v5", "request_id": null, "kind": "log_loss",
            "dropped": {"hook": 0, "access": 0, "subscription": u64::MAX, "notice": 0}
        })
    );
    let counts = Counters::default();
    let mut pipe = pipe::Pipe::start(
        std::path::Path::new("unused"),
        Err(std::io::Error::other("unavailable")),
    );
    pipe.put(
        Record {
            kind: Kind::Subscription,
            bytes: b"record\n".to_vec(),
        },
        &counts,
    );
    pipe.discard(&counts);
    assert_eq!(
        counts.snapshot(),
        Loss {
            subscription: 1,
            ..Loss::default()
        }
    );
}
