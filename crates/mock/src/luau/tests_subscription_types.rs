//! Verify one closed Subscription root type across ordinary and event resolver modules.

use std::sync::Arc;

use chigiri::core::Source;

use super::{Contract, ScriptCandidate, ScriptSet, static_candidates};
use crate::ir::{MockIr, ScriptId};

const NESTED_ROOT: &str = r#"
schema { query: Query subscription: Live }
type Presence { userId: ID! }
interface Channel { presence(teamId: ID!): Presence! }
type Query {
    live: Live!
    optionalLive: Live
    liveList: [Live!]!
    channel: Channel!
}
type Live implements Channel {
    presence(teamId: ID!): Presence!
    cycle: Live
}
"#;

const LIVE_VALUE: &str = "{__typename = 'Live' :: 'Live', presence = {userId = 'user'}}";

fn contract() -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new("schema.graphql", NESTED_ROOT)]).unwrap(),
    ))
    .unwrap()
}

fn script(contract: &Contract, coordinate: &str) -> ScriptId {
    let (owner, field) = coordinate.split_once('.').unwrap();
    let graph = contract.ir().graph();
    let field = graph.types()[graph.type_id(owner).unwrap().0].fields[field];
    ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap(),
    )
}

fn ordinary(contract: &Contract, coordinate: &str, body: &str) -> ScriptCandidate {
    let id = script(contract, coordinate);
    ScriptCandidate {
        script: id,
        source: super::render_module(contract.ir(), id, body).unwrap(),
    }
}

fn subscription(contract: &Contract, coordinate: &str, resolve: &str) -> ScriptCandidate {
    let id = script(contract, coordinate);
    ScriptCandidate {
        script: id,
        source: super::render_subscription_module(
            contract.ir(),
            id,
            &format!(
                "return ctx.pubsub.subscribe.{}({{topic = 'root'}})",
                crate::events::payload::field_type_name(
                    contract.ir(),
                    contract.ir().scripts()[id.0].field
                )
            ),
            Some(resolve),
        )
        .unwrap(),
    }
}

#[test]
fn nested_root_uses_one_type_through_ordinary_abstract_and_event_modules() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let declarations = contract.declarations();
    assert!(declarations.contains("type ChigiriQueryLiveResult = ChigiriLive\n"));
    let optional_name = super::names::resolver(
        contract.ir(),
        script(&contract, "Query.optionalLive"),
        "Result",
    );
    let optional = declarations
        .lines()
        .find(|line| line.contains(&optional_name))
        .unwrap_or_else(|| panic!("missing {optional_name}"));
    assert_eq!(
        optional,
        format!("export type {optional_name} = ChigiriLive | ChigiriNull")
    );
    let list = declarations
        .lines()
        .find(|line| line.contains("ChigiriQueryLiveListResult"))
        .unwrap();
    assert!(list.contains('{') && list.contains("ChigiriLive"));
    assert!(declarations.contains("ChigiriLive & {"));
    let cycle = declarations
        .lines()
        .find(|line| line.contains("ChigiriLiveCycleResult"))
        .unwrap();
    assert!(cycle.ends_with("= ChigiriLive | ChigiriNull"));
    assert!(!declarations.contains("type ChigiriLiveObject ="));

    let mut candidates = static_candidates(contract.ir()).unwrap();
    for candidate in [
        ordinary(&contract, "Query.live", &format!("return {LIVE_VALUE}")),
        ordinary(&contract, "Query.optionalLive", "return ctx.null"),
        ordinary(
            &contract,
            "Query.liveList",
            &format!("return {{{LIVE_VALUE}}}"),
        ),
        ordinary(
            &contract,
            "Query.channel",
            &format!(
                "local result: ChigiriLive = {LIVE_VALUE}\nreturn result :: ChigiriChannelAbstract"
            ),
        ),
        subscription(&contract, "Live.presence", "return parent.presence"),
        subscription(
            &contract,
            "Live.cycle",
            "if parent.cycle == nil then return ctx.null end\nreturn parent.cycle",
        ),
    ] {
        let id = candidate.script.0;
        candidates[id] = candidate;
    }
    ScriptSet::new(contract, candidates).unwrap();
}

#[test]
fn ordinary_root_producer_must_supply_required_subscription_parent_fields() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let candidate = ordinary(
        &contract,
        "Query.live",
        "return {__typename = 'Live' :: 'Live'}",
    );
    assert!(contract.script(candidate).is_err());
}
