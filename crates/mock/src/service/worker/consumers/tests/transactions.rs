//! Prove independent rollback, continuation, fresh context and explicit logging.

use chigiri::core::Value;

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;

const BODY: &str = r#"
seen += 1
assert(seen == 1)
assert(ctx.scope == 'consumer' and ctx.request == ctx.null)
assert(os.time() == 123)
assert(ctx.event.depth == 0 and ctx.event.topic == 'in' and ctx.event.droppedTotal == ctx.null)
local dynamic:any = ctx
assert(dynamic.generator == nil and dynamic.pubsub.subscribe == nil and dynamic.reject == nil)
ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth})
ctx.pubsub.publish.String({topic='out',payload=event})
ctx.log('info','attempt',{value=event})
if event == 'bad' then
    ctx.fail({code='REJECTED'})
end
"#;

#[tokio::test]
async fn ev_05_06_08_fresh_callbacks_rollback_then_continue_without_clients_or_retry() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [
        Spec::new(
            "producer",
            "assert(ctx.scope=='consumer_setup' and ctx.request==ctx.null) assert(os.time()==123) ctx.log('info','setup') return ctx.pubsub.subscribe.String({topic='in'})",
            BODY,
        ),
        Spec::new(
            "audit",
            "return ctx.pubsub.subscribe.String({topic='out'})",
            "ctx.models.Entry:create({id='audit-'..event,value=event,depth=ctx.event.depth,topic=ctx.event.topic})",
        ),
    ];
    let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    let ir = worker.client.ir().clone();
    let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["input"];
    let mut records = Vec::new();
    // Drive accepted inputs without client requests or assumptions about tick
    // delivery. The previous callback must terminate before the next input.
    for (value, terminals) in [("bad", 1), ("good", 2)] {
        crate::publication::test_support::stage_scalar(
            &mut worker.client.committed.lock().unwrap(),
            &ir,
            field,
            "in",
            value,
        );
        records.extend(support::terminals(&logs, terminals).await);
    }
    assert!(support::entry(&worker, "bad", "value").is_none());
    assert!(support::entry(&worker, "audit-bad", "value").is_none());
    assert_eq!(
        support::entry(&worker, "good", "value"),
        Some(Value::from("good"))
    );
    assert_eq!(
        support::entry(&worker, "audit-good", "depth"),
        Some(Value::Int(1))
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r["consumer"] == "producer" && r["outcome"] == "rolled_back")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r["consumer"] == "producer" && r["outcome"] == "committed")
            .count(),
        1
    );
    let explicit = logs.take_hooks();
    assert_eq!(
        explicit
            .iter()
            .filter(|r| r["message"] == "attempt" && r["fields"]["value"] == "bad")
            .count(),
        1
    );
    assert_eq!(
        explicit
            .iter()
            .filter(|r| r["message"] == "attempt" && r["fields"]["value"] == "good")
            .count(),
        1
    );
    assert!(records.iter().all(|r| r.get("fields").is_none()
        && r.get("topic").is_none()
        && r.get("payload").is_none()));
    support::stop(worker).await;
}

#[tokio::test]
async fn ev_06_caught_host_errors_and_dynamic_return_values_rollback_only_their_event() {
    let _case = crate::vm_gate::test_case();
    for invalid in [
        "local dynamic:any=ctx assert(not pcall(function() dynamic.event.depth=0 end))",
        "local invalid:any=function() return nil, 42 end return invalid()",
        "local dynamic:any=ctx assert(not pcall(function() dynamic.pubsub.publish.String({topic='',payload=event}) end))",
        "while true do end",
    ] {
        let logs = SubscriptionLogs::new();
        let body = format!(
            "ctx.models.Entry:create({{id=event,value=event,depth=ctx.event.depth}}) ctx.pubsub.publish.String({{topic='out',payload=event}}) if event=='bad' then {invalid} end"
        );
        let specs = [Spec::new(
            "audit",
            "return ctx.pubsub.subscribe.String({topic='in'})",
            &body,
        )];
        let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
        support::emit(&worker, "in", "bad").await;
        support::terminals(&logs, 1).await;
        assert!(
            support::entry(&worker, "bad", "value").is_none(),
            "{invalid}"
        );
        support::emit(&worker, "in", "good").await;
        let result = support::terminals(&logs, 1).await;
        assert_eq!(
            support::entry(&worker, "good", "value"),
            Some(Value::from("good")),
            "{invalid}: {result:?}"
        );
        assert!(result.iter().any(|r| r["outcome"] == "committed"));
        support::stop(worker).await;
    }
}

#[tokio::test]
async fn ev_02_future_all_topics_are_exact_type_scoped_and_independent() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let body = "assert(ctx.event.topic~=ctx.null and ctx.event.depth==0 and ctx.event.droppedTotal==0) ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth,topic=ctx.event.topic,counter=ctx.event.droppedTotal})";
    let specs = [Spec::new(
        "audit",
        "return ctx.pubsub.subscribe.String({all_topics=true,overflow='drop_oldest'})",
        body,
    )];
    let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    for (topic, value) in [
        ("first", "one"),
        ("future:*", "two"),
        ("future:new", "three"),
    ] {
        support::emit(&worker, topic, value).await;
        support::terminals(&logs, 1).await;
        assert_eq!(
            support::entry(&worker, value, "topic"),
            Some(Value::from(topic))
        );
    }
    support::stop(worker).await;
}
