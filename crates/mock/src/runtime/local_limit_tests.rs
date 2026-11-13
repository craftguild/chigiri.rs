//! Verify resolver-local hard limits remain classified after capture and at exact boundaries.

use super::*;
use crate::limits::request::{BudgetFailure, LocalLimit};

#[test]
fn resolver_memory_host_and_reentry_failures_remain_local_hard_limits() {
    let _case = crate::vm_gate::test_case();
    // Exercise the existing protected-call limit on the production worker stack.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(resolver_limits)
        .unwrap()
        .join()
        .unwrap();
}

fn resolver_limits() {
    let contract = tests::contract("type Query{value:Int!}");
    for (body, expected) in [
        (
            "local s='x' for _=1,30 do s=s..s end return 1",
            Some(LocalLimit::VmMemory),
        ),
        (
            "pcall(function() local s='x' for _=1,30 do s=s..s end end) return 1",
            Some(LocalLimit::VmMemory),
        ),
        ("for _=1,256 do ctx.jsonObject({}) end return 1", None),
        (
            "pcall(function() for _=1,257 do ctx.jsonObject({}) end end) return 1",
            Some(LocalLimit::HostCalls),
        ),
        (
            "local function f(n:number):() if n>0 then pcall(f,n-1) end end f(16) return 1",
            None,
        ),
        (
            "local function f(n:number):() if n>0 then pcall(f,n-1) end end f(17) return 1",
            Some(LocalLimit::PcallReentry),
        ),
    ] {
        let script = tests::script(&contract, "Query.value", body);
        let mut store = Store::empty(contract.ir().clone());
        let snapshot = store.snapshot();
        let execution = Execution {
            state: contract.state().instantiate().snapshot(),
            request: None,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
            cancel: Default::default(),
            shutdown: Default::default(),
            budget: Default::default(),
        };
        let result = execute_root_with(&script, &Object::new(), &mut store, &snapshot, &execution);
        assert_eq!(result.is_err(), expected.is_some(), "{body}");
        assert_eq!(
            execution.budget.failure(),
            expected.map(BudgetFailure::LocalHard),
            "{body}"
        );
        assert_eq!(store.snapshot().size().0, 0);
    }
}

#[test]
fn local_hard_failure_survives_cleanup_cancellation_and_later_errors() {
    for original in [
        None,
        Some(BudgetFailure::Cancelled),
        Some(BudgetFailure::Execution),
        Some(BudgetFailure::Deadline),
        Some(BudgetFailure::LocalHard(LocalLimit::HostCalls)),
    ] {
        let mut work = crate::value::work::Work::default();
        if let Some(cause) = original {
            work.fail_with(cause);
        }
        work.fail_local(LocalLimit::VmMemory);
        work.fail_independent();
        work.fail_with(BudgetFailure::Cancelled);
        let expected = match original {
            None | Some(BudgetFailure::Cancelled) => {
                Some(BudgetFailure::LocalHard(LocalLimit::VmMemory))
            }
            _ => original,
        };
        assert_eq!(work.failure(), expected);
    }
}

#[test]
fn pure_vm_error_text_does_not_misclassify_memory_or_host_failures() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let slot = crate::hooks::HookSlot::RequestBefore;
    for body in [
        "error('not enough memory', 0)",
        "pcall(function() error('not enough memory', 0) end) return nil",
    ] {
        let script = hook_tests::script(&contract, slot, body);
        let call = hook_tests::call(&contract, slot);
        let budget = call.budget.clone();
        assert!(execute_hook(&script, call).is_err());
        assert_eq!(budget.failure(), Some(BudgetFailure::Execution));
    }
}
