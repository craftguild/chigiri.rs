//! Verify stored nullability and patch-presence rules through actual VM execution.

use super::tests::{contract, run, script};
use crate::{ir::ScriptId, luau::ScriptCandidate, output::OutputValue, store::Store};
use chigiri::core::{Object, Value};

const SDL: &str = r#"
type Payload { label:String! }
type Item { id:ID! name:String! note:String payload:Payload! }
type Query { probe:Boolean! }
type Mutation { probe:Boolean! }
"#;

#[test]
fn mr_01_17_model_value_policy_preserves_patch_presence_null_storage_and_typename() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let valid = script(
        &contract,
        "Mutation.probe",
        r#"
        local model = ctx.models.Item
        local row = model:create({id="Item-1",name="before", payload={label="inner"}})
        local kind: "Item" = row.__typename
        local selected: ChigiriItemFindOptions = {where={field="id",op="eq",value=row.id}}
        local unchanged = model:update(selected, {})[1]
        local cleared = model:update(selected, {note=ctx.null})[1]
        return kind == "Item" and unchanged.name == "before"
            and cleared.note == ctx.null and cleared.payload.label == "inner"
    "#,
    );
    assert!(matches!(
        run(&valid, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
    let item = contract
        .ir()
        .model_for(contract.ir().graph().type_id("Item").unwrap())
        .unwrap();
    assert_eq!(store.snapshot().count(item, &Object::new()).unwrap(), 1);
    let mutation = ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|script| script.role.is_mutation_root())
            .unwrap(),
    );
    for body in [
        "ctx.models.Item:create({id='Item-1',payload={label='inner'}}) return true",
        "ctx.models.Item:update({where={field='id',op='eq',value='Item-1'}}, {payload={label=ctx.null}}) return true",
    ] {
        assert!(
            contract
                .script(ScriptCandidate {
                    script: mutation,
                    source: crate::luau::render_module(contract.ir(), mutation, body).unwrap()
                })
                .is_err()
        );
    }
    for field in ["name", "payload"] {
        let body = format!(
            r#"
            local model = ctx.models.Item
            model:create({{id="Item-2",name="rollback",payload={{label="inner"}}}})
            local ok = pcall(function() model:update({{where={{field="id",op="eq",value="Item-1"}}}}, {{{field}=ctx.null}}) end)
            assert(ok == false)
            return true
        "#
        );
        // The type checker sees the dynamic null sentinel; the host rejects it and rolls back the earlier create.
        let candidate = script(&contract, "Mutation.probe", &body);
        assert!(run(&candidate, &Object::new(), &mut store).is_err());
        assert_eq!(store.snapshot().count(item, &Object::new()).unwrap(), 1);
    }
    let recovery = script(
        &contract,
        "Mutation.probe",
        r#"
        local row = ctx.models.Item:create({id="Item-2",name="after",payload={label="ok"}})
        return row.id == "Item-2" and row.__typename == "Item"
    "#,
    );
    assert!(matches!(
        run(&recovery, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
    assert_eq!(store.snapshot().count(item, &Object::new()).unwrap(), 2);
}
