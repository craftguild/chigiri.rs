//! Build checked authoring fixtures and observe actual committed state and logs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use crate::events::{ConsumerRegistration, EventSettings, SourceLimits};
use crate::ir::MockIr;
use crate::logging::test_support::SubscriptionLogs;
use crate::luau::{ConsumerCandidate, Contract, ScriptError, ScriptSet, render_consumer_module};
use crate::service::worker::Worker;
use crate::store::Store;

pub(super) const SDL: &str = r#"
schema {query: Read mutation: Write subscription: Signals}
type Read {entries: [Entry!]! item:Item}
type Entry {id:ID! value:String! depth:Int! topic:String counter:Int}
type Item {id:ID! title:String!}
type Write {emit(value:String!,topic:String!):Int!}
type Signals {input(required:ID!):String! output:String! count:Int! item:Item!}
"#;

pub(super) struct Spec<'a> {
    pub name: &'a str,
    pub field: &'a str,
    pub enabled: bool,
    pub subscribe: &'a str,
    pub consume: &'a str,
}

impl<'a> Spec<'a> {
    pub fn new(name: &'a str, subscribe: &'a str, consume: &'a str) -> Self {
        Self {
            name,
            field: "input",
            enabled: true,
            subscribe,
            consume,
        }
    }
}

pub(super) fn settings(specs: &[Spec<'_>], limits: SourceLimits) -> EventSettings {
    EventSettings::new(
        limits,
        specs
            .iter()
            .map(|spec| {
                (
                    spec.name.to_owned(),
                    ConsumerRegistration {
                        field: spec.field.to_owned(),
                        enabled: spec.enabled,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap()
}

pub(super) fn checked(
    ir: &Arc<MockIr>,
    settings: &EventSettings,
    specs: &[Spec<'_>],
) -> (ScriptSet, Vec<crate::luau::ConsumerScript>) {
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = settings
        .resolve(ir)
        .unwrap()
        .into_iter()
        .map(|registration| {
            let spec = specs
                .iter()
                .find(|spec| spec.name == registration.name())
                .unwrap();
            let source =
                render_consumer_module(ir, registration.field(), spec.subscribe, spec.consume)
                    .unwrap()
                    .replacen("--!strict", "--!strict\nlocal seen = 0", 1);
            ConsumerCandidate {
                registration,
                source,
            }
        })
        .collect();
    let consumers = contract.consumers(candidates, None).unwrap();
    let resolvers = crate::service::tests::candidates(
        ir,
        &[(
            "Write.emit",
            "ctx.pubsub.publish.String({topic=args.topic,payload=args.value}) return 1",
        )],
    );
    (ScriptSet::new(contract, resolvers).unwrap(), consumers)
}

pub(super) fn start(
    specs: &[Spec<'_>],
    limits: SourceLimits,
    logs: &SubscriptionLogs,
) -> Result<Worker, ScriptError> {
    let ir = crate::service::tests::ir(SDL);
    let settings = settings(specs, limits);
    let (scripts, consumers) = checked(&ir, &settings, specs);
    Worker::from_checked_with_events_logged(
        scripts,
        Vec::new(),
        Store::empty(ir),
        crate::clock::ClockSettings::fixed(123).unwrap(),
        settings,
        consumers,
        Some(logs.logger.clone()),
    )
}

pub(super) async fn emit(worker: &Worker, topic: &str, value: &str) {
    let ir = worker.client.ir();
    let field = ir.graph().types()[ir.graph().mutation().unwrap().0].fields["emit"];
    worker
        .client
        .execute(
            field,
            Object::from_iter([("topic", Value::from(topic)), ("value", Value::from(value))]),
            None,
        )
        .await
        .unwrap();
}

pub(super) async fn terminals(logs: &SubscriptionLogs, count: usize) -> Vec<serde_json::Value> {
    let mut records = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            records.extend(logs.take());
            if records
                .iter()
                .filter(|r| r["kind"] == "consumer_terminal")
                .count()
                >= count
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    let observed: Vec<_> = records
        .iter()
        .filter(|r| r["kind"] == "consumer_terminal")
        .map(|r| (&r["consumer"], &r["outcome"], &r["cause"], &r["invocation"]))
        .collect();
    assert!(
        result.is_ok(),
        "consumer terminal watchdog (required {count}): {observed:?}"
    );
    records
}

pub(super) fn entry(worker: &Worker, id: &str, key: &str) -> Option<Value> {
    let ir = worker.client.ir();
    let model = ir
        .models()
        .iter()
        .position(|model| ir.graph().types()[model.ty.0].name.as_ref() == "Entry")
        .unwrap();
    let model = crate::ir::ModelId(model);
    worker
        .client
        .committed
        .lock()
        .unwrap()
        .snapshot()
        .record(model, id)
        .and_then(|record| record.get(ir.models()[model.0].slots[key]).cloned())
}

pub(super) async fn stop(mut worker: Worker) {
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(worker.thread.is_none() && worker.delivery.is_none());
}
