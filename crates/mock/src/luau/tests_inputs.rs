//! Exercise OneOf cardinality and recursive DTO CRUD types in the production checker.

use super::tests::{candidate, contract};
use super::{ScriptSet, static_candidates};

#[test]
fn rsm_08_oneof_requires_exactly_one_nonnull_selection() {
    let _test_case = crate::vm_gate::test_case();
    let contract =
        contract("input Pick @oneOf { id: ID name: String } type Query { x(pick: Pick): String! }");
    for (value, accepted) in [
        ("{id = 'x'}", true),
        ("{name = 'x'}", true),
        ("{}", false),
        ("{id = 'x', name = 'y'}", false),
        ("{id = ctx.null}", false),
    ] {
        let body = format!(
            "local function take(value: ChigiriPickInput): string return 'ok' end\nreturn take({value})"
        );
        assert_eq!(
            contract
                .script(candidate(&contract, "Query.x", &body))
                .is_ok(),
            accepted,
            "{value}"
        );
    }
}

#[test]
fn rsm_08_recursive_dto_crud_and_nullable_model_list_items_are_typed() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type Detail { text:String! child:Detail computed(scale:Int):Int }
        input DetailInput { text:String! child:DetailInput }
        type Item { id:ID! detail:Detail! tags:[String!]! }
        input Create { detail:DetailInput! tags:[String!]! }
        input Patch { detail:DetailInput tags:[String!] }
        type Query { items:[Item]! }
        type Mutation {
            create(input:Create!):Item
            patch(id:ID!,input:Patch!):Item
        }
    "#,
    );
    let candidates = static_candidates(contract.ir()).unwrap();
    let set = ScriptSet::new(contract, candidates).unwrap();
    let contract = set.contract();
    contract
        .script(candidate(contract, "Detail.computed", "return 1"))
        .unwrap();
    contract
        .script(candidate(contract, "Query.items", "return {ctx.null}"))
        .unwrap();
}
