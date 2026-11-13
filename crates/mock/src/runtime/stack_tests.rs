//! Test stack capture at real VM failure sites and omission of stale frames from synthetic errors.

use super::*;

#[test]
fn resolver_host_stack_contains_only_bounded_lines_from_its_module() {
    let _case = crate::vm_gate::test_case();
    let contract = tests::contract("type Query{value:Int!}");
    let script = tests::script(
        &contract,
        "Query.value",
        r#"
local function fail(): any
    local private = 'PRIVATE_STACK_VALUE'
    local bad: any = ctx.jsonObject
    local value = bad(1)
    return value
end
local value = fail()
return value
"#,
    );
    let error = tests::run(
        &script,
        &Object::new(),
        &mut Store::empty(contract.ir().clone()),
    )
    .err()
    .unwrap();
    let stack = error.stack.as_ref().expect("host failure stack");
    assert_eq!(stack.origin, "host");
    assert!(stack.lines.len() >= 2);
    assert!(!stack.truncated);
    assert!(
        stack
            .lines
            .iter()
            .all(|line| *line > 0 && *line <= script.source().lines().count())
    );
    let json = serde_json::to_string(stack).unwrap();
    assert!(!json.contains("PRIVATE_STACK_VALUE"));
    assert!(!json.contains("fail"));
}

#[test]
fn deep_hook_failure_stack_is_truncated_at_the_frame_limit() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let slot = crate::hooks::HookSlot::RequestBefore;
    let script = hook_tests::script(
        &contract,
        slot,
        r#"
local function nested(n: number): any
    if n == 0 then ctx.log('invalid','PRIVATE_STACK_VALUE') return nil end
    local value = nested(n - 1)
    return value
end
local value = nested(24)
return value
"#,
    );
    let error = execute_hook(&script, hook_tests::call(&contract, slot))
        .err()
        .unwrap();
    let stack = error.stack.expect("runtime failure should include a stack");
    assert_eq!(stack.origin, "host");
    assert!(!stack.lines.is_empty());
    assert!(stack.lines.len() <= crate::protected_call::REENTRY_LIMIT);
    assert!(stack.truncated);
}

#[test]
fn deadline_interrupts_capture_a_stack_but_pure_vm_errors_do_not() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let slot = crate::hooks::HookSlot::RequestBefore;
    let pure = hook_tests::script(
        &contract,
        slot,
        "local x:any = {} local y:any = 1 return x + y",
    );
    assert!(
        execute_hook(&pure, hook_tests::call(&contract, slot))
            .err()
            .unwrap()
            .stack
            .is_none()
    );
    let looping = hook_tests::script(&contract, slot, "while true do end return nil");
    let error = execute_hook(&looping, hook_tests::call(&contract, slot))
        .err()
        .unwrap();
    let stack = error
        .stack
        .expect("a deadline interrupt must capture a stack");
    assert_eq!(stack.origin, "interrupt");
    assert!(!stack.lines.is_empty());
}

#[test]
fn only_direct_invalid_rejection_retains_its_captured_stack() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let slot = crate::hooks::HookSlot::RequestBefore;
    for (body, captured) in [
        ("ctx.reject({code=499}) return nil", true),
        (
            "pcall(function() ctx.reject({code=499}) end) return nil",
            false,
        ),
    ] {
        let script = hook_tests::script(&contract, slot, body);
        let HookOutcome::Rejected(rejection, Some(error)) =
            execute_hook(&script, hook_tests::call(&contract, slot)).unwrap()
        else {
            panic!("the invalid rejection diagnostic must be preserved")
        };
        assert!(rejection.invalid);
        assert_eq!(error.stack.is_some(), captured);
        if let Some(stack) = error.stack {
            assert_eq!(stack.origin, "host");
            assert!(!stack.lines.is_empty());
        }
    }
}

#[test]
fn a_captured_host_stack_is_not_reused_for_a_later_pure_vm_error() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let slot = crate::hooks::HookSlot::RequestBefore;
    for caught in [
        "pcall(function() ctx.log('invalid','private') end)",
        "local f:any=nil pcall(f)",
        "local call:any=pcall call()",
    ] {
        let script = hook_tests::script(
            &contract,
            slot,
            &format!("{caught} local x:any = {{}} local y:any = 1 return x + y"),
        );
        let error = execute_hook(&script, hook_tests::call(&contract, slot))
            .err()
            .unwrap();
        assert!(error.stack.is_none(), "{caught}");
    }
}
