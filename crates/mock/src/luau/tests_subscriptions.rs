//! Verify exact subscription module shapes and statically separated setup/publish privileges.

use std::sync::Arc;

use chigiri::core::Source;

use super::module_value::Entrypoint;
use super::{Contract, ScriptCandidate, ScriptSet, static_candidates};
use crate::ir::{MockIr, ScriptId, ScriptRole};

const SOURCE_ONLY: &str = r#"
schema { query: Query subscription: Subscription }
type Query { ping: Int }
type Presence { userId: ID! online: Boolean! }
type Subscription { presence(teamId: ID!): Presence! }
"#;

const NESTED: &str = r#"
schema { query: Query mutation: Mutation subscription: Subscription }
type Presence { userId: ID! online: Boolean! }
type Query { subscription: Subscription! }
type Mutation { setPresence(teamId: ID!, userId: ID!, online: Boolean!): Presence! }
type Subscription { presence(teamId: ID!): Presence! }
"#;

fn make_contract(source: &str) -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new("schema.graphql", source)]).unwrap(),
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

fn candidate(script: ScriptId, source: &str) -> ScriptCandidate {
    ScriptCandidate {
        script,
        source: source.to_owned(),
    }
}

#[test]
fn subscription_source_only_requires_exact_subscribe_module() {
    let _case = crate::vm_gate::test_case();
    let contract = make_contract(SOURCE_ONLY);
    let id = script(&contract, "Subscription.presence");
    assert_eq!(
        contract.ir().scripts()[id.0].role,
        ScriptRole::SubscriptionRoot {
            root_as_nested: false
        }
    );
    let generated = static_candidates(contract.ir()).unwrap();
    let source = &generated[id.0].source;
    assert!(source.contains("subscribe = function("));
    assert!(!source.contains("resolve = function("));
    ScriptSet::new(contract, generated).unwrap();

    let contract = make_contract(SOURCE_ONLY);
    for source in [
        "return {}",
        "return {resolve=function() return nil end}",
        "return {subscribe=true}",
        "return {subscribe=function() return nil end,extra=function() end}",
        "return {subscribe=function() return nil end,resolve=function() return nil end}",
    ] {
        assert!(contract.script(candidate(id, source)).is_err(), "{source}");
    }
}

#[test]
fn nested_subscription_requires_typed_subscribe_and_resolve() {
    let _case = crate::vm_gate::test_case();
    let contract = make_contract(NESTED);
    let id = script(&contract, "Subscription.presence");
    assert!(contract.ir().scripts()[id.0].role.root_as_nested());
    let source = super::render_subscription_module(
        contract.ir(),
        id,
        "return ctx.pubsub.subscribe.Presence({topic = 'team:' .. args.teamId})",
        Some("return parent.presence"),
    )
    .unwrap();
    let script = contract.script(candidate(id, &source)).unwrap();
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let length = crate::sandbox::string_length(&lua).unwrap();
    let subscribe = super::module_value::load_entrypoint_observed(
        &lua,
        &length,
        &script,
        Entrypoint::Subscribe,
        |_| false,
    );
    assert!(subscribe.is_ok());
    let resolve = super::module_value::load_entrypoint_observed(
        &lua,
        &length,
        &script,
        Entrypoint::Resolve,
        |_| false,
    );
    assert!(resolve.is_ok());
}

#[test]
fn publish_is_typed_and_available_only_to_mutation_context() {
    let _case = crate::vm_gate::test_case();
    let contract = make_contract(NESTED);
    let mutation = script(&contract, "Mutation.setPresence");
    let body = "ctx.pubsub.publish.Presence({topic='team:' .. args.teamId,payload={userId=args.userId,online=args.online}})\nreturn {userId=args.userId,online=args.online}";
    contract
        .script(candidate(
            mutation,
            &super::render_module(contract.ir(), mutation, body).unwrap(),
        ))
        .unwrap();
    let wrong_payload = "ctx.pubsub.publish.Presence({topic='x',payload={userId=42,online='yes'}})\nreturn {userId=args.userId,online=args.online}";
    assert!(
        contract
            .script(candidate(
                mutation,
                &super::render_module(contract.ir(), mutation, wrong_payload).unwrap(),
            ))
            .is_err()
    );

    let query = script(&contract, "Query.subscription");
    let invalid = "ctx.pubsub.publish.Presence({topic='x',payload={userId='x',online=true}})\nreturn ctx.fail({code='NOT_IMPLEMENTED'})";
    assert!(
        contract
            .script(candidate(
                query,
                &super::render_module(contract.ir(), query, invalid).unwrap(),
            ))
            .is_err()
    );
    let subscription = script(&contract, "Subscription.presence");
    let invalid_overflow = super::render_subscription_module(
        contract.ir(),
        subscription,
        "return ctx.pubsub.subscribe.Presence({topic='x',overflow='drop_newest'})",
        Some("return parent.presence"),
    )
    .unwrap();
    assert!(
        contract
            .script(candidate(subscription, &invalid_overflow))
            .is_err()
    );
    let invalid_setup = super::render_subscription_module(
        contract.ir(),
        subscription,
        "ctx.pubsub.publish.Presence({topic='x',payload={userId='x',online=true}})\nreturn ctx.pubsub.subscribe.Presence({topic='x'})",
        Some("return parent.presence"),
    )
    .unwrap();
    assert!(
        contract
            .script(candidate(subscription, &invalid_setup))
            .is_err()
    );
    let invalid_event = super::render_subscription_module(
        contract.ir(),
        subscription,
        "return ctx.pubsub.subscribe.Presence({topic='x'})",
        Some("ctx.pubsub.publish.Presence({topic='x',payload=parent.presence})\nreturn parent.presence"),
    )
    .unwrap();
    assert!(
        contract
            .script(candidate(subscription, &invalid_event))
            .is_err()
    );
    assert!(
        contract
            .script(candidate(
                subscription,
                "return {subscribe=function(ctx: ChigiriSubscriptionPresenceSetupContext, args: ChigiriSubscriptionPresenceArgs): ChigiriSubscriptionPresenceSource ctx.pubsub.publish.Presence({topic='x',payload={userId='x',online=true}}) return ctx.pubsub.subscribe.Presence({topic='x'}) end,resolve=function(ctx: ChigiriReadContext,args:ChigiriSubscriptionPresenceArgs,parent:ChigiriSubscription):ChigiriPresence ctx.pubsub.publish.Presence({topic='x',payload=parent.presence}) return parent.presence end}",
            ))
            .is_err()
    );
}

#[test]
fn ordinary_contracts_do_not_gain_subscription_types_or_privileges() {
    let _case = crate::vm_gate::test_case();
    let contract = make_contract("type Query { ping: Int } type Mutation { set: Int }");
    assert!(
        !contract
            .declarations()
            .contains("declare extern type ChigiriSubscription")
    );
    assert!(!contract.declarations().contains("[\"publish\"]"));
    let candidates = static_candidates(contract.ir()).unwrap();
    assert!(candidates.iter().all(|candidate| {
        candidate.source.contains("resolve = function(")
            && !candidate.source.contains("subscribe = function(")
    }));
    ScriptSet::new(contract, candidates).unwrap();
}

#[test]
fn subscription_generated_names_reject_case_collisions() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "schema.graphql",
            "type Query{x:Int} type Subscription{presence:Int Presence:Int}",
        )])
        .unwrap(),
    );
    assert_eq!(Contract::new(ir).err().unwrap().phase, "contract");
}

#[test]
fn source_only_custom_root_does_not_reserve_a_nested_result_alias() {
    let _case = crate::vm_gate::test_case();
    let contract = make_contract(
        "schema { query: Query subscription: SubscriptionFooResult } type Query { x: Int } type SubscriptionFooResult { foo: Int }",
    );
    let id = script(&contract, "SubscriptionFooResult.foo");
    assert_eq!(
        contract.ir().scripts()[id.0].role,
        ScriptRole::SubscriptionRoot {
            root_as_nested: false
        }
    );
    assert_eq!(
        contract
            .declarations()
            .matches("type ChigiriSubscriptionFooResult =")
            .count(),
        1
    );
    let candidates = static_candidates(contract.ir()).unwrap();
    assert!(candidates[id.0].source.contains("subscribe = function("));
    assert!(!candidates[id.0].source.contains("resolve = function("));
    ScriptSet::new(contract, candidates).unwrap();
}
