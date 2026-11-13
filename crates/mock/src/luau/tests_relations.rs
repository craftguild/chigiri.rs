//! Type-check owner-side keys and inverse relation results in nested queries.

use super::tests::{candidate, contract};

#[test]
fn mr_01_14_relation_signatures_use_exact_owner_names_and_scope() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type Owner {id:ID! end:[Target!]! @relation}
        type Target {id:ID! owners:[Owner!]! @relation}
        type Query {probe:Boolean!}
        type Mutation {probe:Boolean!}
    "#,
    );
    for body in [
        "ctx.models.Owner:connect('a', 'targetIds', 'b') return true",
        "ctx.models.Owner:disconnect('a', 'missing', 'b') return true",
        "ctx.models.Owner:connect('a', 'end', 1) return true",
        "ctx.models.Owner:connect('a', 'end') return true",
        "ctx.models.Owner:disconnect('a', 'end', 'b', 'extra') return true",
        "ctx.models.Target:connect('b', 'owners', 'a') return true",
        "ctx.models.Target:disconnect('b', 'owners', 'a') return true",
    ] {
        assert!(
            contract
                .script(candidate(&contract, "Mutation.probe", body))
                .is_err(),
            "{body}"
        );
    }
    let connect = "local source: ChigiriOwnerFindOptions = {where={field='id',op='eq',value='a'}} local target: ChigiriTargetFindOptions = {where={field='id',op='eq',value='b'}} ctx.models.Owner:connect(source, 'end', target) return true";
    let disconnect = "local source: ChigiriOwnerFindOptions = {where={field='id',op='eq',value='a'}} local target: ChigiriTargetFindOptions = {where={field='id',op='eq',value='b'}} ctx.models.Owner:disconnect(source, 'end', target) return true";
    for body in [connect, disconnect] {
        contract
            .script(candidate(&contract, "Mutation.probe", body))
            .unwrap();
        assert!(
            contract
                .script(candidate(&contract, "Query.probe", body))
                .is_err()
        );
    }
}
