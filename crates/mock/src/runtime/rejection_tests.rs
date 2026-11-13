//! Distinguish rejection unwinds, preserve independent captured errors, and enforce hard deadlines.

use super::hook_tests::{call, contract, script};
use super::{HookOutcome, execute_hook};
use crate::hooks::HookSlot;
use crate::limits::request::{BudgetFailure, LimitKind, LimitNotice};
use std::sync::Arc;

#[test]
fn rejection_accepts_closed_statuses_and_rejects_invalid_arguments() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::RequestBefore;
    let statuses = [
        200, 400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411, 412, 413, 414, 415, 416,
        417, 421, 422, 423, 424, 425, 426, 428, 429, 431, 451, 500, 501, 502, 503, 504, 505, 506,
        507, 508, 510, 511,
    ];
    for status in statuses {
        let body = format!("ctx.reject({{code={status}}})");
        let result = execute_hook(&script(&contract, slot, &body), call(&contract, slot)).unwrap();
        let HookOutcome::Rejected(reason, _) = result else {
            panic!("a valid rejection must be returned")
        };
        assert_eq!(reason.status, status);
        assert!(!reason.invalid);
        assert_eq!(reason.message, "Request rejected");
    }
    for arguments in [
        "",
        "nil",
        "true",
        "42",
        "'private'",
        "{}",
        "{code=403},{}",
        "{code=403,extra=true}",
        "{code=403,message=true}",
        "{code=403,message=ctx.null}",
    ] {
        let body = format!("pcall(function() ctx.reject({arguments}) end) return nil");
        let input = call(&contract, slot);
        let budget = input.budget.clone();
        let result = execute_hook(&script(&contract, slot, &body), input);
        if matches!(arguments, "nil" | "true" | "42" | "'private'") {
            // Values that cannot form a rejection fail conversion independently and remain failures through `pcall`.
            assert!(result.is_err(), "{arguments}");
            assert_eq!(budget.failure(), Some(BudgetFailure::Execution));
            continue;
        }
        let HookOutcome::Rejected(reason, _) =
            result.unwrap_or_else(|error| panic!("{arguments}: {error:?}"))
        else {
            panic!("an invalid rejection must remain a rejection")
        };
        assert_eq!(reason.status, 500, "{arguments}");
        assert!(reason.invalid, "{arguments}");
        assert_eq!(reason.message, "Internal server error");
    }
    // Exercise the rejection-message byte boundary through both codec and rejection decoding.
    for (bytes, valid) in [
        (crate::value::MAX_STRING_BYTES, true),
        (crate::value::MAX_STRING_BYTES + 1, false),
    ] {
        let body = format!(
            "pcall(function() ctx.reject({{code=403,message='{}'}}) end) return nil",
            "x".repeat(bytes)
        );
        let hook = script(&contract, slot, &body);
        let input = call(&contract, slot);
        let budget = input.budget.clone();
        let result = execute_hook(&hook, input);
        if valid {
            let HookOutcome::Rejected(reason, _) = result.unwrap_or_else(|error| {
                panic!("bytes={bytes}, failure={:?}, {error:?}", budget.failure())
            }) else {
                panic!("a valid bounded message must be returned as a rejection")
            };
            assert!(!reason.invalid);
            assert_eq!(reason.message.len(), bytes);
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn caught_hook_errors_remain_execution_failures() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for slot in HookSlot::ALL {
        let hook = script(
            &contract,
            slot,
            "pcall(function() error('private failure') end) return nil",
        );
        let call = call(&contract, slot);
        let budget = call.budget.clone();
        let error = execute_hook(&hook, call).err().unwrap();
        assert!(!error.cancelled);
        assert_eq!(budget.failure(), Some(BudgetFailure::Execution));
    }
}

#[test]
fn pre_execution_failures_distinguish_contract_cancellation_and_deadline() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::FieldBefore;
    let hook = script(&contract, slot, "return nil");
    for (mode, expected) in [
        (0, BudgetFailure::Execution),
        (1, BudgetFailure::Cancelled),
        (2, BudgetFailure::Deadline),
    ] {
        let mut call = call(&contract, slot);
        match mode {
            0 => call.views.clear(),
            1 => call.cancel.cancel(),
            _ => call.deadline = std::time::Instant::now() - std::time::Duration::from_millis(1),
        }
        let budget = call.budget.clone();
        let error = execute_hook(&hook, call).err().unwrap();
        assert_eq!(error.cancelled, mode == 1);
        assert_eq!(budget.failure(), Some(expected));
    }
}

#[test]
fn direct_and_caught_rejections_succeed_but_independent_errors_win() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::OperationBefore;
    for body in [
        "ctx.reject({code=200})",
        "pcall(function() ctx.reject({code=200}) end) return nil",
        "pcall(function() pcall(function() ctx.reject({code=200}) end) end) return nil",
        "for _=1,256 do pcall(function() ctx.reject({code=200}) end) end return nil",
    ] {
        let result = execute_hook(&script(&contract, slot, body), call(&contract, slot));
        assert!(
            matches!(result, Ok(HookOutcome::Rejected(rejection, None)) if rejection.status == 200)
        );
    }
    for body in [
        "pcall(function() ctx.reject({code=200}) end) error('private failure')",
        "pcall(function() pcall(function() ctx.reject({code=200}) end) end) error('private failure')",
        "pcall(function() ctx.reject({code=200}) end) ctx.log('unknown','bad')",
        "for _=1,257 do pcall(function() ctx.reject({code=200}) end) end return nil",
    ] {
        assert!(execute_hook(&script(&contract, slot, body), call(&contract, slot)).is_err());
    }
}

#[test]
fn hard_limits_and_deadlines_cannot_be_hidden_by_rejection() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::OperationBefore;
    let hook = script(
        &contract,
        slot,
        "pcall(function() ctx.reject({code=200}) end) ctx.log('info','charge') return nil",
    );
    let mut call = call(&contract, slot);
    let budget = call.budget.clone();
    let work = budget.0.clone();
    call.log = Arc::new(move |_| {
        let _ = work.clone().charge(usize::MAX);
    });
    assert!(execute_hook(&hook, call).is_err());
    assert!(matches!(
        budget.failure(),
        Some(BudgetFailure::Hard(LimitNotice {
            kind: LimitKind::Work,
            ..
        }))
    ));
    let hook = script(
        &contract,
        slot,
        "pcall(function() ctx.reject({code=200}) end) while true do end return nil",
    );
    let call = super::hook_tests::call(&contract, slot);
    let budget = call.budget.clone();
    assert!(execute_hook(&hook, call).is_err());
    assert_eq!(budget.failure(), Some(BudgetFailure::Deadline));
}

#[test]
fn direct_rejection_from_a_generated_module_is_a_control_unwind() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::FieldBefore;
    let hook = contract
        .hooks(
            vec![crate::luau::HookCandidate {
                slot,
                source: crate::luau::render_hook_module(
                    slot,
                    "ctx.reject({code=403,message='Denied'}) return nil",
                )
                .unwrap(),
            }],
            None,
        )
        .unwrap()
        .remove(0);
    assert!(matches!(execute_hook(&hook, call(&contract, slot)),
        Ok(HookOutcome::Rejected(reason, None)) if reason.status == 403));
}

#[test]
fn completed_rejection_is_not_converted_to_an_internal_failure() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::FieldBefore;
    let hook = script(
        &contract,
        slot,
        "pcall(function() ctx.reject({code=403,message='Denied'}) end) ctx.log('info','cancel') return nil",
    );
    let mut call = call(&contract, slot);
    let token = call.cancel.clone();
    let budget = call.budget.clone();
    call.log = Arc::new(move |_| token.cancel());
    assert!(matches!(
        execute_hook(&hook, call),
        Ok(HookOutcome::Rejected(rejection, None)) if rejection.status == 403
    ));
    assert_eq!(budget.failure(), Some(BudgetFailure::Cancelled));
}

#[test]
fn cancellation_and_caught_host_errors_preserve_their_distinct_causes() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::FieldBefore;
    for (body, cancelled, expected) in [
        (
            "ctx.log('info','cancel') ctx.log('info','interrupted') return nil",
            true,
            BudgetFailure::Cancelled,
        ),
        (
            "pcall(function() ctx.log('info','cancel') ctx.log('info','interrupted') end) local x:any = {} local y:any = 1 return x + y",
            false,
            BudgetFailure::Execution,
        ),
    ] {
        let module = script(&contract, slot, body);
        let mut call = call(&contract, slot);
        let token = call.cancel.clone();
        let budget = call.budget.clone();
        call.log = Arc::new(move |_| token.cancel());
        let error = execute_hook(&module, call).err().unwrap();
        assert_eq!(error.cancelled, cancelled, "{body}");
        assert_eq!(budget.failure(), Some(expected), "{body}");
    }
}
