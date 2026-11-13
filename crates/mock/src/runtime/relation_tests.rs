//! Relation editing, type/arity checks, and full rollback through the real VM.

use super::tests::{contract, run, script};
use crate::output::OutputValue;
use crate::store::Store;
use chigiri::core::{Object, Value};

const SDL: &str = r#"
type Person {
  id:ID! groups:[Group!]! @relation(fields:["groupIds"],references:["id"])
  partner:Person @relation(fields:["partnerId"],references:["id"])
}
type Group {id:ID! members:[Person!]! @relation}
type Query {probe:Boolean!}
type Mutation {edit:Boolean!}
"#;

#[test]
fn mr_01_10_14_real_vm_exposes_seven_methods_for_both_relation_sides() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let candidate = script(
        &contract,
        "Mutation.edit",
        r#"
        local expected = {find = true, count = true, create = true, update = true,
            delete = true, connect = true, disconnect = true}
        local registry: {any} = {ctx.models.Person, ctx.models.Group}
        for _, model in ipairs(registry) do
            local count = 0
            for key, method in pairs(model) do
                assert(expected[key] and type(method) == 'function')
                count += 1
            end
            assert(count == 7)
        end
        local g = ctx.models.Group:create({id='Group-1'})
        local p = ctx.models.Person:create({id='Person-1',groupIds = {}})
        local person: ChigiriPersonFindOptions = {where={field='id',op='eq',value=p.id}}
        local group: ChigiriGroupFindOptions = {where={field='id',op='eq',value=g.id}}
        local connected = ctx.models.Person:connect(person, 'groups', group)[1]
        assert(connected.id == p.id and connected.__typename == 'Person')
        assert(#connected.groupIds == 1 and connected.groupIds[1] == g.id)
        assert(#p.groupIds == 0)
        local same = ctx.models.Person:connect(person, 'groups', group)[1]
        assert(#same.groupIds == 1)
        local inverse = ctx.models.Group:connect(group, 'members', person)
        assert(#inverse == 1 and inverse[1].id == g.id)
        assert(#ctx.models.Group:disconnect(group, 'members', person) == 1)
        local cleared = ctx.models.Person:disconnect(person, 'groups', group)[1]
        assert(#cleared.groupIds == 0)
        assert(#connected.groupIds == 1)
        assert(ctx.models.Person:connect(person, 'partner', person)[1].partnerId == p.id)
        assert(ctx.models.Person:disconnect(person, 'partner', person)[1].partnerId == ctx.null)
        return true
    "#,
    );
    assert!(matches!(
        run(&candidate, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
    let read = script(
        &contract,
        "Query.probe",
        r#"
        local count = 0
        for key, method in pairs(ctx.models.Person) do
            assert((key == 'find' or key == 'count') and type(method) == 'function')
            count += 1
        end
        return count == 2 and ctx.models.Person:count({}) == 1
    "#,
    );
    assert!(matches!(
        run(&read, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn mr_13_14_real_vm_caught_relation_failure_rolls_back_models_indices_and_registration() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    for code in [
        "m:connect(person, 'groups')",
        "m:disconnect(person, 'groups', group, 'extra')",
        "m.connect(ctx.models.Group, person, 'groups', group)",
        "m:connect(person, 'groupIds', group)",
        "m:connect(person, 'groups', {where={field='missing',op='eq',value=1}})",
        "m:disconnect({offset=ctx.null}, 'groups', group)",
        "m:disconnect(person, 'groups', ctx.null)",
        "m:disconnect(person, {}, group)",
        "m:connect(person, 'groups', 1)",
        "m:connect(person, 'partner', {where={field='id',op='eq',value='Person-2'}})",
        "m:connect('Person-1', 'groups', 'Group-1')",
    ] {
        let body = format!(
            r#"
            local g = ctx.models.Group:create({{id='Group-1'}})
            local a = ctx.models.Person:create({{id='Person-1',groupIds = {{}}}})
            ctx.models.Person:create({{id='Person-2',groupIds = {{}}}})
            local person: ChigiriPersonFindOptions = {{where={{field='id',op='eq',value=a.id}}}}
            local group: ChigiriGroupFindOptions = {{where={{field='id',op='eq',value=g.id}}}}
            ctx.models.Person:connect(person, 'groups', group)
            ctx.models.Person:connect(person, 'partner', person)
            local m = ctx.models.Person :: any
            pcall(function() {code} end)
            return true
        "#
        );
        let candidate = script(&contract, "Mutation.edit", &body);
        assert!(
            run(&candidate, &Object::new(), &mut store).is_err(),
            "{code}"
        );
        assert_eq!(store.snapshot().size().0, 0);
    }
    let after = script(
        &contract,
        "Mutation.edit",
        r#"
        local g = ctx.models.Group:create({id='Group-1'})
        local p = ctx.models.Person:create({id='Person-1',groupIds = {}})
        local person: ChigiriPersonFindOptions = {where={field='id',op='eq',value=p.id}}
        local group: ChigiriGroupFindOptions = {where={field='id',op='eq',value=g.id}}
        ctx.models.Person:connect(person, 'groups', group)
        ctx.models.Person:disconnect(person, 'groups', group)
        assert(ctx.models.Group:delete(group) == 1)
        return p.id == 'Person-1' and g.id == 'Group-1'
    "#,
    );
    assert!(matches!(
        run(&after, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}
