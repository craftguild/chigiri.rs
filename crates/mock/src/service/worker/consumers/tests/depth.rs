//! Causal authority follows immutable inputs across fields and topics through depth three.

use chigiri::core::Value;

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;

#[tokio::test]
async fn ev_07_three_edges_commit_and_depth_four_poisoning_survives_a_catch() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let bodies: Vec<_> = [("a", "b", 0), ("b", "c", 1), ("c", "d", 2)].map(|(id, next, depth)| {
        format!("assert(ctx.event.depth=={depth}) ctx.models.Entry:create({{id='{id}',value=event,depth=ctx.event.depth}}) ctx.pubsub.publish.String({{topic='{next}',payload=event}})")
    }).into();
    let mut specs = Vec::new();
    for (index, name) in ["a", "b", "c"].into_iter().enumerate() {
        specs.push(Spec::new(
            name,
            [
                "return ctx.pubsub.subscribe.String({topic='a'})",
                "return ctx.pubsub.subscribe.String({topic='b'})",
                "return ctx.pubsub.subscribe.String({topic='c'})",
            ][index],
            &bodies[index],
        ));
    }
    specs.push(Spec::new("d", "return ctx.pubsub.subscribe.String({topic='d'})", "assert(ctx.event.depth==3) ctx.models.Entry:create({id='d',value=event,depth=ctx.event.depth})"));
    specs.push(Spec::new("reject", "return ctx.pubsub.subscribe.String({topic='d'})", "assert(ctx.event.depth==3) ctx.models.Entry:create({id='reject',value=event,depth=ctx.event.depth}) assert(not pcall(function() ctx.pubsub.publish.Int({topic='cross-field',payload=1}) end))"));
    let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    support::emit(&worker, "a", "secret-payload").await;
    let records = support::terminals(&logs, 5).await;
    for (id, depth) in [("a", 0), ("b", 1), ("c", 2), ("d", 3)] {
        assert_eq!(
            support::entry(&worker, id, "depth"),
            Some(Value::Int(depth))
        );
    }
    assert!(support::entry(&worker, "reject", "depth").is_none());
    assert!(
        records
            .iter()
            .any(|r| r["consumer"] == "reject" && r["outcome"] == "rolled_back")
    );
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("secret-payload")
    );
    support::stop(worker).await;
}

#[tokio::test]
async fn ev_07_self_cycle_is_bounded_by_depth_without_retry_or_queue_drain() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [Spec::new(
        "cycle",
        "return ctx.pubsub.subscribe.String({topic='cycle'})",
        "ctx.models.Entry:create({id=tostring(ctx.event.depth),value=event,depth=ctx.event.depth}) ctx.pubsub.publish.String({topic='cycle',payload=event})",
    )];
    let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    support::emit(&worker, "cycle", "root").await;
    let records = support::terminals(&logs, 4).await;
    for depth in 0..3 {
        assert_eq!(
            support::entry(&worker, &depth.to_string(), "depth"),
            Some(Value::Int(depth))
        );
    }
    assert!(support::entry(&worker, "3", "depth").is_none());
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "committed")
            .count(),
        3
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "rolled_back")
            .count(),
        1
    );
    support::stop(worker).await;
}
