//! Consumer descriptors keep field identity, phase privileges and module isolation.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::Source;

use super::{ConsumerCandidate, Contract, render_consumer_module, static_consumers};
use crate::events::{ConsumerRegistration, EventSettings, SourceLimits};
use crate::ir::MockIr;

fn contract() -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new(
            "consumer.graphql",
            r#"
schema {query: Read mutation: Write subscription: Signals}
type Read {ping:Int!}
type Write {emit:Int!}
type Item {id:ID! title:String!}
type Signals {count(required:ID!):Int! other:Int! maybe:Int texts:[String!]! item:Item!}
"#,
        )])
        .unwrap(),
    ))
    .unwrap()
}

fn settings(enabled: bool) -> EventSettings {
    EventSettings::new(
        SourceLimits::default(),
        BTreeMap::from([(
            "audit".into(),
            ConsumerRegistration {
                field: "count".into(),
                enabled,
            },
        )]),
    )
    .unwrap()
}

fn candidate(
    contract: &Contract,
    subscribe: &str,
    consume: &str,
    enabled: bool,
) -> ConsumerCandidate {
    let registration = settings(enabled).resolve(contract.ir()).unwrap().remove(0);
    let source =
        render_consumer_module(contract.ir(), registration.field(), subscribe, consume).unwrap();
    ConsumerCandidate {
        registration,
        source,
    }
}

#[test]
fn ev_01_05_consumer_roles_use_own_sources_null_request_and_typed_execution() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for select in [
        "ctx.pubsub.subscribe.Int({topic='literal:*'})",
        "ctx.pubsub.subscribe.Int({all_topics=true,overflow='drop_oldest'})",
        "ctx.generator.sequence.Int({values={1,2},interval_ms=1})",
    ] {
        let candidate = candidate(
            &contract,
            &format!(
                "local request:ChigiriNull = ctx.request ctx.log('info','selecting') return {select}"
            ),
            "local request:ChigiriNull = ctx.request local topic:string | ChigiriNull = ctx.event.topic local depth:number = ctx.event.depth local dropped:number | ChigiriNull = ctx.event.droppedTotal ctx.models.Item:create({id='one',title='written'}) ctx.pubsub.publish.Int({topic='out',payload=event}) ctx.log('info','received')",
            true,
        );
        assert!(!candidate.source.contains("Args"));
        let original = candidate.source.clone();
        let consumers = contract
            .consumers(vec![candidate], None)
            .unwrap_or_else(|error| {
                let _lease = crate::vm_gate::test_guard();
                let observed = luaur::check_with_definitions(&original, contract.declarations());
                drop(_lease);
                panic!("{select}: {error:?}\n{observed:?}\n{original}")
            });
        assert_eq!(consumers[0].source(), original);
        assert_eq!(consumers[0].registration().name(), "audit");
        assert!(consumers[0].registration().enabled());
    }
    let mut renamed = candidate(
        &contract,
        "return ctx.pubsub.subscribe.Int({all_topics=true,overflow='drop_oldest'})",
        "",
        false,
    );
    renamed.source = renamed.source.replace("ctx", "host");
    contract.consumers(vec![renamed], None).unwrap();
}

#[test]
fn ev_01_05_consumers_reject_cross_field_sources_and_wrong_phase_authority() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for (subscribe, consume) in [
        ("return ctx.pubsub.subscribe.Item({topic='x'})", ""),
        ("return ctx.pubsub.subscribe.Int({all_topics=false})", ""),
        (
            "return ctx.pubsub.subscribe.Int({all_topics=true,overflow='drop_newest'})",
            "",
        ),
        (
            "return ctx.pubsub.subscribe.Int({topic='x',all_topics=true})",
            "",
        ),
        (
            "ctx.models.Item:create({id='one',title='bad'}) return ctx.pubsub.subscribe.Int({topic='x'})",
            "",
        ),
        (
            "ctx.pubsub.publish.Int({topic='x',payload=1}) return ctx.pubsub.subscribe.Int({topic='x'})",
            "",
        ),
        ("return {}", ""),
        (
            "return ctx.pubsub.subscribe.Int({topic='x'})",
            "ctx.generator.sequence.Int({values={1},interval_ms=1})",
        ),
        (
            "return ctx.pubsub.subscribe.Int({topic='x'})",
            "ctx.pubsub.subscribe.Int({topic='x'})",
        ),
        (
            "return ctx.pubsub.subscribe.Int({topic='x'})",
            "ctx.pubsub.publish.Int({topic='x',payload='wrong'})",
        ),
        (
            "return ctx.pubsub.subscribe.Int({topic='x'})",
            "local request:ChigiriHookRequest = ctx.request",
        ),
    ] {
        // A disabled registration still receives both function-body checks.
        let candidate = candidate(&contract, subscribe, consume, false);
        let error = contract.consumers(vec![candidate], None).err().unwrap();
        assert_eq!(error.consumer.as_deref(), Some("audit"));
        assert_eq!(error.phase, "type", "{subscribe} / {consume}: {error:?}");
    }
}

#[test]
fn ev_05_consumer_descriptors_are_closed_and_callbacks_never_run_during_check() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let candidates = static_consumers(contract.ir(), &settings(false)).unwrap();
    assert!(candidates[0].source.contains("NOT_IMPLEMENTED"));
    assert!(!candidates[0].source.contains("state ="));
    contract.consumers(candidates, None).unwrap();
    let callbacks = candidate(
        &contract,
        "error('subscribe must not execute during check')",
        "error('consume must not execute during check')",
        true,
    );
    contract.consumers(vec![callbacks], None).unwrap();
    for descriptor in [
        "nil",
        "{subscribe=function() end}",
        "{consume=function() end}",
        "{subscribe=function() end,consume=true}",
        "{subscribe=function() end,consume=function() end,resolve=function() end}",
        "{subscribe=function() end,consume=function() end,extra=true}",
    ] {
        let mut candidate = candidate(&contract, "error('unused')", "", false);
        candidate.source = format!("local descriptor:any = {descriptor}\nreturn descriptor");
        let error = contract.consumers(vec![candidate], None).err().unwrap();
        assert_eq!(error.phase, "module", "{descriptor}: {error:?}");
        assert_eq!(error.consumer.as_deref(), Some("audit"));
    }
}

#[test]
fn ev_05_consumer_aggregate_capacity_precedes_candidate_syntax() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let registrations = (0..400)
        .map(|index| {
            (
                format!("consumer_{index}"),
                ConsumerRegistration {
                    field: "count".into(),
                    enabled: false,
                },
            )
        })
        .collect();
    let settings = EventSettings::new(SourceLimits::default(), registrations).unwrap();
    let source = format!("invalid @\n--{}", "x".repeat(31_000));
    let candidates = settings
        .resolve(contract.ir())
        .unwrap()
        .into_iter()
        .map(|registration| ConsumerCandidate {
            registration,
            source: source.clone(),
        })
        .collect();
    let error = contract.consumers(candidates, None).err().unwrap();
    assert_eq!(error.phase, "limit");
    assert!(error.consumer.is_some());
}
