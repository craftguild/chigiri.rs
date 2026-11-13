//! Field identity and role checks use generated nominal types, not payload shape.

use std::sync::Arc;

use chigiri::core::Source;

use super::{
    Contract, ScriptCandidate, render_module, render_subscription_module, static_candidates,
};
use crate::ir::{MockIr, ScriptId};

const SDL: &str = r#"
schema { query: Read mutation: Write subscription: Signals }
type Read { ping: Int! }
type Write { emit: Int! }
type Item { id: ID! title: String! }
type Signals {
    count: Int!
    other: Int!
    maybe: Int
    texts: [String!]!
    item: Item!
}
"#;

pub(super) fn contract() -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new("schema.graphql", SDL)]).unwrap(),
    ))
    .unwrap()
}

pub(super) fn id(contract: &Contract, coordinate: &str) -> ScriptId {
    ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|plan| {
                let field = &contract.ir().graph().fields()[plan.field.0];
                format!(
                    "{}.{}",
                    contract.ir().graph().types()[field.parent.0].name,
                    field.name
                ) == coordinate
            })
            .unwrap(),
    )
}

#[test]
fn ev_01_each_custom_root_field_has_a_dot_call_and_own_nominal_source() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    assert!(
        contract
            .declarations()
            .contains("declare extern type ChigiriSignalsCountSource")
    );
    for field in ["count", "other", "maybe", "texts", "item"] {
        let id = id(&contract, &format!("Signals.{field}"));
        let selector = crate::events::payload::field_type_name(
            contract.ir(),
            contract.ir().scripts()[id.0].field,
        );
        let source = render_subscription_module(
            contract.ir(),
            id,
            &format!(
                "local select = ctx.pubsub.subscribe.{selector}\nreturn select({{topic = 'room:*'}})"
            ),
            None,
        )
        .unwrap();
        contract
            .script(ScriptCandidate { script: id, source })
            .unwrap();
    }
    assert!(
        !contract
            .declarations()
            .contains("ChigiriSubscriptionBinding")
    );
    assert!(
        !contract
            .declarations()
            .contains("ChigiriSubscriptionSetupContext")
    );
    let _lease = crate::vm_gate::test_guard();
    for source in [
        "local function wrong(source: ChigiriSignalsCountSource): ChigiriSignalsOtherSource return source end",
        "local source: ChigiriSignalsCountSource = {}",
        "local function wrong(null: ChigiriNull): ChigiriSignalsCountSource return null end",
    ] {
        assert!(
            luaur::check_with_definitions(source, contract.declarations()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn ev_01_setup_can_select_only_its_field_and_cannot_use_old_names_or_publish() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let id = id(&contract, "Signals.count");
    for body in [
        "return ctx.pubsub.subscribe.Item({topic='x'})",
        "return ctx.pubsub.subscribe.Int({topic=1})",
        "ctx.pubsub.publish.Int({topic='x',payload=1}) return ctx.pubsub.subscribe.Int({topic='x'})",
        "return ctx.subscription:bind({topic='x'})",
    ] {
        let source = render_subscription_module(contract.ir(), id, body, None).unwrap();
        assert!(
            contract
                .script(ScriptCandidate { script: id, source })
                .is_err(),
            "{body}"
        );
    }
}

#[test]
fn ev_01_publication_types_keep_scalar_nullable_list_and_record_contracts() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let mutation = id(&contract, "Write.emit");
    let body = "ctx.pubsub.publish.Int({topic='x',payload=1})\nctx.pubsub.publish.Int({topic='x',payload=ctx.null})\nctx.pubsub.publish.String({topic='x',payload={'a','b'}})\nlocal item=ctx.models.Item:create({id='one',title='ok'})\nctx.pubsub.publish.Item({topic='x',payload=item})\nreturn 1";
    let source = render_module(contract.ir(), mutation, body).unwrap();
    contract
        .script(ScriptCandidate {
            script: mutation,
            source,
        })
        .unwrap();
    for invalid in [
        "ctx.pubsub.publish.Int({topic='x',payload='wrong'})",
        "ctx.pubsub.publish.String({topic='x',payload='a'})",
        "ctx.pubsub.publish.Item({topic='x',payload={id=1,title='bad'}})",
        "ctx.publish.Subscription.count({topic='x',payload=1})",
        "ctx.pubsub.subscribe.Int({topic='x'})",
    ] {
        let source =
            render_module(contract.ir(), mutation, &format!("{invalid}\nreturn 1")).unwrap();
        assert!(
            contract
                .script(ScriptCandidate {
                    script: mutation,
                    source
                })
                .is_err(),
            "{invalid}"
        );
    }
    // Intentional NOT_IMPLEMENTED sources remain statically valid and do not
    // acquire a completion flag or run their entrypoints during validation.
    let candidates = static_candidates(contract.ir()).unwrap();
    super::ScriptSet::new(contract, candidates).unwrap();
}
