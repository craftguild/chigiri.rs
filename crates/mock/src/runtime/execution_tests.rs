//! Verifies that hooks determine resolver state fields while snapshots and
//! deadlines remain shared across real reads and commits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use super::hook_tests::{call, contract, script as hook};
use super::tests::{input, run, script};
use super::{Execution, HookOutcome, execute_hook, execute_root_with};
use crate::cancellation::Cancellation;
use crate::hooks::HookSlot;
use crate::output::OutputValue;
use crate::store::Store;

#[test]
fn resolver_divides_and_differentiates_between_input_failures_and_cancellations_and_deadlines() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let resolver = script(
        &contract,
        "Query.getUsers",
        "return ctx.models.User:find({})",
    );
    for (mode, expected) in [
        (0, crate::limits::request::BudgetFailure::Execution),
        (1, crate::limits::request::BudgetFailure::Cancelled),
        (2, crate::limits::request::BudgetFailure::Deadline),
        (3, crate::limits::request::BudgetFailure::Execution),
    ] {
        let mut store = Store::empty(contract.ir().clone());
        let mut execution = Execution {
            state: contract.state().instantiate().snapshot(),
            request: None,
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Cancellation::default(),
            shutdown: Cancellation::default(),
            budget: Default::default(),
        };
        let mut arguments = Object::from_iter([("limit", Value::Int(20))]);
        let mut snapshot = store.snapshot();
        match mode {
            0 => arguments = Object::new(),
            1 => execution.cancel.cancel(),
            2 => execution.deadline = Instant::now() - Duration::from_millis(1),
            _ => snapshot = Store::empty(super::hook_tests::contract().ir().clone()).snapshot(),
        }
        assert!(
            execute_root_with(&resolver, &arguments, &mut store, &snapshot, &execution).is_err()
        );
        assert_eq!(execution.budget.failure(), Some(expected), "mode={mode}");
        assert_eq!(store.snapshot().size().0, 0);
    }
}

#[test]
fn resolver_exception_and_return_conversion_failure_remain_in_the_shared_ledger() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for body in [
        "error('private resolver failure')",
        "local invalid: any = true return invalid",
    ] {
        let mut store = Store::empty(contract.ir().clone());
        let resolver = script(&contract, "Query.getUsers", body);
        let execution = Execution {
            state: contract.state().instantiate().snapshot(),
            request: None,
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Cancellation::default(),
            shutdown: Cancellation::default(),
            budget: Default::default(),
        };
        let snapshot = store.snapshot();
        let arguments = Object::from_iter([("limit", Value::Int(20))]);
        let error = execute_root_with(&resolver, &arguments, &mut store, &snapshot, &execution)
            .err()
            .unwrap();
        assert_eq!(
            execution.budget.failure(),
            Some(crate::limits::request::BudgetFailure::Execution),
            "{body}: {error}"
        );
        assert_eq!(store.snapshot().size().0, 0);
    }
}

#[test]
fn hk_04_11_14_resolver_uses_patched_state_selected_snapshot_and_original_deadline() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let mut store = Store::empty(contract.ir().clone());
    let create = script(
        &contract,
        "Mutation.createUser",
        "return ctx.models.User:create({id=args.input.name,name=args.input.name})",
    );
    run(&create, &input("before"), &mut store).unwrap();
    let before = store.snapshot();
    run(&create, &input("later"), &mut store).unwrap();
    let module = hook(
        &contract,
        HookSlot::OperationBefore,
        "return {statePatch={authenticated=true,count=7}} ",
    );
    let HookOutcome::Continue(Some(state)) =
        execute_hook(&module, call(&contract, HookSlot::OperationBefore)).unwrap()
    else {
        panic!("state update required")
    };
    let query = script(
        &contract,
        "Query.getUsers",
        "assert(ctx.state.authenticated) assert(ctx.state.count==7) assert(ctx.state.name=='Example') return ctx.models.User:find({})",
    );
    let mut execution = Execution {
        state: state.snapshot(),
        request: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        budget: Default::default(),
    };
    let arguments = Object::from_iter([("limit", Value::Int(20))]);
    let output = execute_root_with(&query, &arguments, &mut store, &before, &execution).unwrap();
    assert!(matches!(output, OutputValue::List(rows) if rows.len()==1));
    let error = execute_root_with(&create, &input("stale"), &mut store, &before, &execution)
        .err()
        .unwrap();
    assert_eq!(error.phase, "contract");
    assert_eq!(store.snapshot().size().0, 2);
    let current = store.snapshot();
    execution.deadline = Instant::now() - Duration::from_millis(1);
    assert!(
        execute_root_with(&create, &input("expired"), &mut store, &current, &execution).is_err()
    );
    assert_eq!(store.snapshot().size().0, 2);
    assert_eq!(
        state.snapshot().get("authenticated"),
        Some(&Value::Boolean(true))
    );
    assert_eq!(
        contract.state().initial().get("authenticated"),
        Some(&Value::Boolean(false))
    );
    assert!(Arc::ptr_eq(&state.snapshot(), &execution.state));
}

#[test]
fn hk_03_resolver_initial_state_and_readonly_nested_values_share_declared_contract() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let mut store = Store::empty(contract.ir().clone());
    let create = script(
        &contract,
        "Mutation.createUser",
        "assert(not ctx.state.authenticated) assert(ctx.state.profile.name=='Original') return ctx.models.User:create({id=args.input.name,name=args.input.name})",
    );
    run(&create, &input("first"), &mut store).unwrap();
    let bad = script(
        &contract,
        "Mutation.createUser",
        "local c:any=ctx c.models.User:create({id=args.input.name,name=args.input.name}) c.state.profile.name='forged' return c.models.User:find({})[1]",
    );
    assert!(run(&bad, &input("rollback"), &mut store).is_err());
    assert_eq!(store.snapshot().size().0, 1);
    let current = store.snapshot();
    let execution = Execution {
        state: contract.state().instantiate().snapshot(),
        request: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        budget: Default::default(),
    };
    execute_root_with(&create, &input("next"), &mut store, &current, &execution).unwrap();
    assert_eq!(store.snapshot().size().0, 2);
}
