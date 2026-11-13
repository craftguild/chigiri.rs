//! Graceful retains one begun candidate and abandons unstarted recursive work.

use std::time::{Duration, Instant};

use chigiri::core::Value;

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;

#[tokio::test]
async fn ev_11_graceful_finishes_only_the_running_input_without_draining_children() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [
        Spec::new(
            "first",
            "return ctx.pubsub.subscribe.String({topic='in'})",
            r#"
ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth})
ctx.log('info','entered')
local until_time = os.clock() + 0.15
while os.clock() < until_time do end
ctx.pubsub.publish.String({topic='out',payload=event})
"#,
        ),
        Spec::new(
            "child",
            "return ctx.pubsub.subscribe.String({topic='out'})",
            "ctx.models.Entry:create({id='child-'..event,value=event,depth=ctx.event.depth})",
        ),
    ];
    let mut worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    let permit = worker.client.mutation.clone().lock_owned().await;
    let ir = worker.client.ir().clone();
    let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["input"];
    for value in ["running", "waiting"] {
        crate::publication::test_support::stage_scalar(
            &mut worker.client.committed.lock().unwrap(),
            &ir,
            field,
            "in",
            value,
        );
    }
    drop(permit);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !logs.take_hooks().iter().any(|r| r["message"] == "entered") {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(worker.client.events_running());
    worker.client.quiesce_events();
    assert!(!worker.client.shutdown.cancelled());
    let retention = worker.client.committed.lock().unwrap().source_policy().1;
    worker
        .stop_graceful(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(
        support::entry(&worker, "running", "value"),
        Some(Value::from("running"))
    );
    assert!(support::entry(&worker, "waiting", "value").is_none());
    assert!(support::entry(&worker, "child-running", "value").is_none());
    let records = logs.take();
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "committed")
            .count(),
        1
    );
    assert!(!records.iter().any(|r| r["outcome"] == "rolled_back"));
    assert_eq!(retention.used(), 0);
}
