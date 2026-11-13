//! Admission leaves inputs evictable until invocation and cleanup retains actual charges.

use std::time::{Duration, Instant};

use chigiri::core::Value;

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;

#[tokio::test]
async fn ev_09_10_write_contention_keeps_inputs_waiting_and_isolates_source_overflow() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [
        Spec::new(
            "stopped",
            "return ctx.pubsub.subscribe.String({topic='in'})",
            "ctx.models.Entry:create({id='stopped-'..event,value=event,depth=ctx.event.depth})",
        ),
        Spec::new(
            "dropping",
            "return ctx.pubsub.subscribe.String({topic='in',overflow='drop_oldest'})",
            "ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth,counter=ctx.event.droppedTotal})",
        ),
    ];
    let limits = SourceLimits::new(2, 256 * 1024, 10_000, 16 * 1024 * 1024, 1024 * 1024).unwrap();
    let mut worker = support::start(&specs, limits, &logs).unwrap();
    let permit = worker.client.mutation.clone().lock_owned().await;
    let ir = worker.client.ir().clone();
    let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["input"];
    let retention = worker.client.committed.lock().unwrap().source_policy().1;
    for value in ["one", "two", "three"] {
        crate::publication::test_support::stage_scalar(
            &mut worker.client.committed.lock().unwrap(),
            &ir,
            field,
            "in",
            value,
        );
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while !worker.client.publication_stage_idle_for_test() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        logs.take().is_empty(),
        "waiting sources create no invocation or terminal log"
    );
    assert!(support::entry(&worker, "two", "value").is_none());
    drop(permit);
    let records = support::terminals(&logs, 3).await;
    assert!(support::entry(&worker, "one", "value").is_none());
    for id in ["two", "three"] {
        assert_eq!(support::entry(&worker, id, "counter"), Some(Value::Int(1)));
        assert!(support::entry(&worker, &format!("stopped-{id}"), "value").is_none());
    }
    assert_eq!(
        records
            .iter()
            .filter(|r| r["consumer"] == "stopped" && r["outcome"] == "stopped")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r["consumer"] == "dropping" && r["outcome"] == "committed")
            .count(),
        2
    );
    worker
        .stop(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(
        retention.used(),
        0,
        "every source input, definition and staging charge is released"
    );
}

#[tokio::test]
async fn ev_11_immediate_shutdown_cancels_the_running_candidate_and_abandons_waiting_inputs() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [Spec::new(
        "audit",
        "return ctx.pubsub.subscribe.String({topic='in'})",
        "ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth}) ctx.pubsub.publish.String({topic='out',payload=event}) ctx.log('info','entered') while true do end",
    )];
    let mut worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    support::emit(&worker, "in", "cancelled").await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !logs.take_hooks().iter().any(|r| r["message"] == "entered") {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let retention = worker.client.committed.lock().unwrap().source_policy().1;
    worker
        .stop(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert!(support::entry(&worker, "cancelled", "value").is_none());
    let records = logs.take();
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "rolled_back")
            .count(),
        1
    );
    assert!(!records.iter().any(|r| r["outcome"] == "committed"));
    assert_eq!(retention.used(), 0);
}
