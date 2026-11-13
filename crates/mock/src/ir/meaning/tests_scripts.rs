//! Verify that CRUD-like names do not imply behavior and that manual fields require scripts.

use std::sync::Arc;

use chigiri::core::Source;

use super::tests::parse;
use crate::ir::{FieldPlan, MockIr, ScriptId, ScriptRole};
use crate::luau::{Contract, ScriptSet, static_candidates};

#[test]
fn sd_04_root_and_argument_fields_are_manual_without_business_mapping() {
    let _case = crate::vm_gate::test_case();
    let ir = Arc::new(
        parse(
            r#"
        input Unrelated { arbitrary:Boolean nested:Unrelated }
        type Payload { text:String formatted(prefix:String):String computed:Int @virtual }
        type Query { create(input:Unrelated):Int find(filter:Boolean):Payload }
        type Mutation { find(other:String):Boolean }
    "#,
        )
        .unwrap(),
    );
    assert_eq!(ir.scripts().len(), 5);
    assert_eq!(
        ir.scripts()
            .iter()
            .filter(|s| s.role.is_mutation_root())
            .count(),
        1
    );
    let payload = &ir.graph().types()[ir.graph().type_id("Payload").unwrap().0];
    assert!(matches!(
        ir.field_plan(payload.fields["formatted"]),
        Some(FieldPlan::Script(_))
    ));
    assert!(matches!(
        ir.field_plan(payload.fields["computed"]),
        Some(FieldPlan::Script(_))
    ));
    assert!(!matches!(
        ir.field_plan(payload.fields["text"]),
        Some(FieldPlan::Script(_))
    ));
    let candidates = static_candidates(&ir).unwrap();
    assert!(
        candidates
            .iter()
            .all(|c| c.source.contains("NOT_IMPLEMENTED"))
    );
    let set = ScriptSet::new(Contract::new(ir).unwrap(), candidates).unwrap();
    assert_eq!(set.scripts().len(), 5);
}

#[test]
fn sd_04_missing_modules_and_wrong_returns_remain_pre_start_errors() {
    let _case = crate::vm_gate::test_case();
    let ir = Arc::new(parse("type Query{find(input:Int):Int}").unwrap());
    assert!(ScriptSet::new(Contract::new(ir.clone()).unwrap(), vec![]).is_err());
    let mut candidates = static_candidates(&ir).unwrap();
    candidates[0].source =
        crate::luau::render_module(&ir, ScriptId(0), "return 'wrong type'\n").unwrap();
    assert!(ScriptSet::new(Contract::new(ir.clone()).unwrap(), candidates).is_err());
    let mut candidates = static_candidates(&ir).unwrap();
    candidates[0].source = crate::luau::render_module(&ir, ScriptId(0), "return 42\n").unwrap();
    ScriptSet::new(Contract::new(ir).unwrap(), candidates).unwrap();
}

#[test]
fn sd_04_virtual_requires_no_arguments_and_root_nested_permissions_are_retained() {
    assert!(parse("type P{x(arg:Int):Int @virtual} type Query{p:P}").is_err());
    let ir = parse("type Query{again:Mutation} type Mutation{reset:Query}").unwrap();
    assert_eq!(ir.scripts().len(), 2);
    assert!(ir.scripts().iter().all(|s| s.role.root_as_nested()));
    assert_eq!(
        ir.scripts()
            .iter()
            .filter(|s| s.role.is_mutation_root())
            .count(),
        1
    );
}

#[test]
fn subscription_root_is_scripted_but_never_inferred_as_a_model() {
    let ir = parse(
        "type Query { value: Int } type Subscription { tick(step: Int): Int nested: Subscription }",
    )
    .unwrap();
    let subscription = ir.graph().subscription().unwrap();
    assert!(ir.model_for(subscription).is_none());
    let roles = ir
        .scripts()
        .iter()
        .filter(|plan| ir.graph().fields()[plan.field.0].parent == subscription)
        .map(|plan| plan.role)
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        vec![
            ScriptRole::SubscriptionRoot {
                root_as_nested: true
            },
            ScriptRole::SubscriptionRoot {
                root_as_nested: true
            }
        ]
    );
}

#[test]
fn storage_cannot_extend_the_public_subscription_root() {
    let error = MockIr::parse_with_storage(
        vec![Source::new(
            "schema.graphql",
            "schema { query: Query subscription: Subscription } type Query { value: Int } type Subscription { tick: Int }",
        )],
        vec![Source::new(
            "storage.graphql",
            "extend type Subscription { secret: String }",
        )],
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "invalid private storage field");
    let diagnostic = format!("{error:?} {error}");
    assert!(!diagnostic.contains("secret"));
    assert!(!diagnostic.contains("storage.graphql"));
}
