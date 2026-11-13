//! Verify shared API registration and arity against independent values in an actual VM.

use super::tests::{contract, run, script};
use crate::output::OutputValue;
use crate::store::Store;
use chigiri::core::{Object, Value};

const SDL: &str = r#"
    type Item { id: ID! name: String! }
    type Other { id: ID! }
    type Query { probe: Boolean! }
    type Mutation { probe: Boolean! }
"#;

#[test]
fn mr_01_shared_contract_preserves_registration_and_crud_semantics() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let mutation = script(
        &contract,
        "Mutation.probe",
        r#"
        local model = ctx.models.Item
        local expected = {find = true, count = true, create = true, update = true, delete = true}
        local count = 0
        for key, value in pairs(model) do
            assert(expected[key] == true and type(value) == 'function')
            count += 1
        end
        assert(count == 5)
        local a = model:create({id = 'Item-1', name = 'first'})
        local b = model:create({id = 'Item-2', name = 'second'})
        assert(a.id == 'Item-1' and b.id == 'Item-2')
        local selected: ChigiriItemFindOptions = {where={field='id',op='eq',value=a.id :: string}}
        local changed = model:update(selected, {name = 'changed'})[1]
        assert(changed.id == a.id and changed.name == 'changed')
        assert(a.name == 'first')
        local rows = model:find({})
        assert(#rows == 2 and rows[1].id == a.id and rows[2].id == b.id)
        assert(rows[1].name == 'changed')
        assert(model:delete(selected) == 1)
        assert(model:delete(selected) == 0)
        return true
    "#,
    );
    assert!(matches!(
        run(&mutation, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
    let query = script(
        &contract,
        "Query.probe",
        r#"
        local count = 0
        for key, value in pairs(ctx.models.Item) do
            assert((key == 'find' or key == 'count') and type(value) == 'function')
            count += 1
        end
        assert(count == 2)
        local rows = ctx.models.Item:find({})
        return #rows == 1 and rows[1].name == 'second'
    "#,
    );
    assert!(matches!(
        run(&query, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn mr_01_14_dynamic_bad_arity_receiver_and_argument_remain_sticky() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    for operation in [
        "model:find()",
        "model:find({}, {})",
        "model.find(ctx.models.Other, {})",
        "model:find('bad')",
        "model:create()",
        "model:create({}, {})",
        "model:update({})",
        "model:update({}, {}, {})",
        "model:delete()",
        "model:delete({}, 'extra')",
        "model:update('Item-1', {})",
        "model:delete('Item-1')",
    ] {
        let body = format!(
            "local model = ctx.models.Item :: any\nmodel:create({{id = 'Item-1', name = 'rollback'}})\npcall(function() {operation} end)\nreturn true"
        );
        let candidate = script(&contract, "Mutation.probe", &body);
        assert!(
            run(&candidate, &Object::new(), &mut store).is_err(),
            "{operation}"
        );
        assert_eq!(store.snapshot().size().0, 0, "{operation}");
    }
}

#[test]
fn mr_01_find_defaults_and_boundaries_are_enforced_in_real_vm() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let populate = script(
        &contract,
        "Mutation.probe",
        r#"
        for i = 1, 101 do ctx.models.Item:create({id = 'Item-' .. i, name = 'x'}) end
        assert(#ctx.models.Item:find({}) == 101)
        assert(#ctx.models.Item:find({limit = 100}) == 100)
        assert(#ctx.models.Item:find({limit = 9007199254740991}) == 101)
        assert(#ctx.models.Item:find({offset = 101}) == 0)
        assert(#ctx.models.Item:find({limit = 0}) == 0)
        return true
    "#,
    );
    run(&populate, &Object::new(), &mut store).unwrap();
    for options in [
        "{limit = -1}",
        "{limit = 9007199254740992}",
        "{limit = 0.5}",
        "{unknown = true}",
    ] {
        let body = format!(
            "local model = ctx.models.Item :: any\npcall(function() model:find({options}) end)\nreturn true"
        );
        assert!(
            run(
                &script(&contract, "Query.probe", &body),
                &Object::new(),
                &mut store
            )
            .is_err()
        );
    }
}
