//! Republish the captured root through real fresh callbacks after same-ID replacement.

use chigiri::core::Value;

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;
use crate::luau::{ConsumerCandidate, Contract, ScriptSet, render_consumer_module};
use crate::service::worker::Worker;
use crate::store::Store;

#[tokio::test]
async fn ev_08_republished_records_keep_original_values_and_a_fresh_current_model_view() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let specs = [
        Spec {
            name: "bridge",
            field: "item",
            enabled: true,
            subscribe: "return ctx.pubsub.subscribe.Item({topic='original'})",
            consume: "assert(event.title=='captured') assert(ctx.models.Item:find({})[1].title=='replacement') ctx.pubsub.publish.Item({topic='child',payload=event}) ctx.models.Entry:create({id='bridge',value=event.title,depth=ctx.event.depth})",
        },
        Spec {
            name: "reader",
            field: "item",
            enabled: true,
            subscribe: "return ctx.pubsub.subscribe.Item({topic='child'})",
            consume: "assert(ctx.models.Item:find({})[1].title=='replacement') ctx.models.Entry:create({id='reader',value=event.title,depth=ctx.event.depth})",
        },
    ];
    let ir = crate::service::tests::ir(support::SDL);
    let events = support::settings(&specs, SourceLimits::default());
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = events
        .resolve(&ir)
        .unwrap()
        .into_iter()
        .map(|registration| {
            let spec = specs
                .iter()
                .find(|spec| spec.name == registration.name())
                .unwrap();
            let source =
                render_consumer_module(&ir, registration.field(), spec.subscribe, spec.consume)
                    .unwrap();
            ConsumerCandidate {
                registration,
                source,
            }
        })
        .collect();
    let consumers = contract.consumers(candidates, None).unwrap();
    let resolvers = crate::service::tests::candidates(
        &ir,
        &[(
            "Write.emit",
            r#"
local original = ctx.models.Item:create({id='same',title='captured'})
ctx.pubsub.publish.Item({topic='original',payload=original})
ctx.models.Item:delete({where={field='id',op='eq',value='same'}})
ctx.models.Item:create({id='same',title='replacement'})
return 1
"#,
        )],
    );
    let scripts = ScriptSet::new(contract, resolvers).unwrap();
    let worker = Worker::from_checked_with_events_logged(
        scripts,
        Vec::new(),
        Store::empty(ir),
        crate::clock::ClockSettings::fixed(123).unwrap(),
        events,
        consumers,
        Some(logs.logger.clone()),
    )
    .unwrap();
    support::emit(&worker, "unused", "unused").await;
    let records = support::terminals(&logs, 2).await;
    assert!(records.iter().all(|r| r["outcome"] == "committed"));
    assert_eq!(
        support::entry(&worker, "bridge", "value"),
        Some(Value::from("captured"))
    );
    assert_eq!(
        support::entry(&worker, "reader", "value"),
        Some(Value::from("captured"))
    );
    assert_eq!(
        support::entry(&worker, "reader", "depth"),
        Some(Value::Int(1))
    );
    support::stop(worker).await;
}
