//! Exercise all seven model operations, relation-bound search types, host checks, and rollback.

use chigiri::core::{Object, Value};

use super::tests::{contract, run, script};
use crate::output::OutputValue;
use crate::store::Store;

const SDL: &str = r#"
    type Source {
        id: Int!
        rank: Int! @constraint(min:0)
        targets: [Target!]! @relation(fields:["targetIds"], references:["id"])
        others: [Other!]! @relation
    }
    type Target { id: Int! tag: String! sources: [Source!]! @relation }
    type Other { id: Int! flag: Boolean! sources: [Source!]! @relation }
    type Query { read: Int! }
    type Mutation { run: Int! }
"#;

#[test]
fn sd_api_vm_seven_methods_use_selection_types_and_inverse_target_options() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let script = script(
        &contract,
        "Mutation.run",
        r#"
        for index = 1, 120 do
            ctx.models.Source:create({id = index, rank = index, targetIds = {}})
        end
        ctx.models.Target:create({id = 1, tag = "one"})
        ctx.models.Other:create({id = 1, flag = true})
        local all = ctx.models.Source:find({})
        if #all ~= 120 then error("all rows required") end
        local changed = ctx.models.Source:update({where = {field = "rank", op = "gte", value = 100}}, {rank = 0})
        if #changed ~= 21 then error("selection changed during update") end
        local byTag: ChigiriTargetFindOptions = {where = {field = "tag", op = "eq", value = "one"}}
        local byFlag: ChigiriOtherFindOptions = {where = {field = "flag", op = "eq", value = true}}
        local linked = ctx.models.Source:connect({where = {field = "rank", op = "eq", value = 0}}, "targets", byTag)
        if #linked ~= 21 then error("source copies required") end
        ctx.models.Source:connect({}, "others", byFlag)
        local unlinked = ctx.models.Target:disconnect({where = {field = "tag", op = "eq", value = "one"}}, "sources", {where = {field = "rank", op = "eq", value = 0}})
        if #unlinked ~= 1 then error("inverse source copies required") end
        local removed = ctx.models.Source:delete({where = {field = "rank", op = "eq", value = 0}})
        if removed ~= 21 then error("delete count required") end
        return ctx.models.Source:count({})
    "#,
    );
    let mut store = Store::empty(contract.ir().clone());
    let output = run(&script, &Object::new(), &mut store).unwrap();
    assert!(matches!(output, OutputValue::Scalar(Value::Int(99))));
    assert_eq!(store.snapshot().size().0, 101);
}

#[test]
fn sd_api_zero_match_bad_patch_or_wrong_target_field_is_sticky_after_pcall() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    for failure in [
        "model:update({limit = 0}, {rank = -1})",
        "model:update({limit = 0}, {id = 1})",
        "model:connect({limit = 0}, 'targets', {where = {field = 'rank', op = 'eq', value = 1}})",
        "model:connect({limit = 0}, 'missing', {limit = 0})",
        "model:update('1', {rank = 1})",
        "model:delete('1')",
        "model:connect('1', 'targets', '1')",
    ] {
        let script = script(
            &contract,
            "Mutation.run",
            &format!(
                r#"
            ctx.models.Source:create({{id = 1, rank = 1, targetIds = {{}}}})
            ctx.models.Target:create({{id = 1, tag = "one"}})
            local model = ctx.models.Source :: any
            pcall(function() {failure} end)
            return 1
        "#
            ),
        );
        let mut store = Store::empty(contract.ir().clone());
        assert!(
            run(&script, &Object::new(), &mut store).is_err(),
            "{failure}"
        );
        assert_eq!(store.snapshot().size().0, 0);
    }
}

#[test]
fn sd_api_relation_overloads_reject_target_fields_from_another_relation() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let field =
        contract.ir().graph().types()[contract.ir().graph().mutation().unwrap().0].fields["run"];
    let id = crate::ir::ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|script| script.field == field)
            .unwrap(),
    );
    for body in [
        "local target: ChigiriOtherFindOptions = {where = {field = 'flag', op = 'eq', value = true}}; return #ctx.models.Source:connect({}, 'targets', target)",
        "local target: ChigiriTargetFindOptions = {where = {field = 'tag', op = 'eq', value = 'one'}}; return #ctx.models.Source:connect({}, 'others', target)",
    ] {
        let candidate = crate::luau::ScriptCandidate {
            script: id,
            source: crate::luau::render_module(contract.ir(), id, body).unwrap(),
        };
        assert!(contract.script(candidate).is_err(), "{body}");
    }
}

#[test]
fn sd_scope_mutation_candidate_copy_uses_request_budget_before_starting_vm() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID!} type Query{x:Int} type Mutation{run:Int!}");
    let script = script(&contract, "Mutation.run", "return 7");
    let mut store =
        Store::from_seed_json(contract.ir().clone(), r#"{"Item":[{"id":"a"},{"id":"b"}]}"#)
            .unwrap();
    let mut budget = super::ExecutionBudget::default();
    budget.0.charge(crate::api::WORK_LIMIT - 5).unwrap();
    let execution = super::Execution {
        state: script.initial_state(),
        request: None,
        deadline: std::time::Instant::now() + super::VM_TIME,
        cancel: Default::default(),
        shutdown: Default::default(),
        budget,
    };
    let snapshot = store.snapshot();
    let error =
        super::execute_root_with(&script, &Object::new(), &mut store, &snapshot, &execution)
            .err()
            .unwrap();
    assert_eq!(error.phase, "budget");
    assert!(execution.budget.0.check().is_err());
    assert_eq!(store.snapshot().size().0, 2);
    assert!(matches!(
        run(&script, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Int(7))
    ));
}

#[test]
fn sa_02_protected_resolver_calls_charge_the_shared_request_work_budget() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query{value:Int!}");
    let script = script(
        &contract,
        "Query.value",
        "for _=1,600 do pcall(function() return 1 end) end return 7",
    );
    let mut store = Store::empty(contract.ir().clone());
    let mut budget = super::ExecutionBudget::default();
    budget.0.charge(crate::api::WORK_LIMIT - 512).unwrap();
    let execution = super::Execution {
        state: script.initial_state(),
        request: None,
        deadline: std::time::Instant::now() + super::VM_TIME,
        cancel: Default::default(),
        shutdown: Default::default(),
        budget,
    };
    let snapshot = store.snapshot();
    let error =
        super::execute_root_with(&script, &Object::new(), &mut store, &snapshot, &execution)
            .err()
            .unwrap();
    assert_eq!(error.phase, "runtime");
    assert!(execution.budget.0.check().is_err());
}
