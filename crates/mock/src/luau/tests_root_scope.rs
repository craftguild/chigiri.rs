//! Verify root types used as nested results and type-check the parent at the same coordinate.

use super::tests::{candidate, contract};
use super::{Contract, ScriptSet, static_candidates};

#[test]
fn rsm_08_nested_root_contract_requires_scope_refinement_and_nullable_parent() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        schema { query: Read mutation: Write }
        type Item { id: ID! name: String! }
        union Roots = Read | Write
        type Read { root: Roots! @virtual }
        type Write { create(flag: Boolean): Item! }
    "#,
    );
    assert!(
        contract
            .ir()
            .scripts()
            .iter()
            .all(|plan| plan.role.root_as_nested())
    );
    contract
        .script(candidate(
            &contract,
            "Write.create",
            "return ctx.models.Item:find({limit=1})[1]",
        ))
        .unwrap();
    for body in [
        "return ctx.models.Item:create({name='unchecked'})",
        "if ctx.scope == 'read' then return ctx.models.Item:create({name='forbidden'}) end error('x')",
        "return parent.missing",
    ] {
        assert!(
            contract
                .script(candidate(&contract, "Write.create", body))
                .is_err()
        );
    }
    let body = r#"
if ctx.scope == 'mutation' then
    assert(parent == nil)
    return ctx.models.Item:create({name='root'})
else
    assert(parent ~= nil)
    return ctx.models.Item:find({limit=1})[1]
end
"#;
    contract
        .script(candidate(&contract, "Write.create", body))
        .unwrap();
}

#[test]
fn rsm_08_static_scaffold_on_nested_root_keeps_typed_contract() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        schema { query: Read mutation: Write }
        type Item { id: ID! name: String! }
        input New { name: String! }
        type Read {
            root: Read!
            mutation: Write!
            note: String!
        }
        type Write {
            create(input: New!): Item!
            peek: Item
        }
    "#,
    );
    let candidates = static_candidates(contract.ir()).unwrap();
    let set = ScriptSet::new(Contract::new(contract.ir().clone()).unwrap(), candidates).unwrap();
    let create = set
        .scripts()
        .iter()
        .find(|script| script.coordinate() == "Write.create")
        .unwrap();
    assert!(create.source().contains("NOT_IMPLEMENTED"));
    assert!(!create.source().contains(": any"));
}
