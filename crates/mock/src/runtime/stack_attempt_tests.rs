//! Bound repeated failures after stack capture and stack exhaustion in one VM.

use super::{
    hook_tests,
    state::{Host, Session},
};
use crate::hooks::HookSlot;
use std::{cell::RefCell, rc::Rc};

#[test]
fn repeated_caught_host_failures_capture_at_most_sixteen_stack_attempts() {
    let _case = crate::vm_gate::test_case();
    let contract = hook_tests::contract();
    let call = hook_tests::call(&contract, HookSlot::RequestBefore);
    let budget = call.budget.clone();
    let mut reader = call.snapshot.reader();
    reader.share_work(budget.0.clone());
    let state = Rc::new(RefCell::new(Host {
        ir: call.snapshot.ir().clone(),
        session: Session::Read(reader),
        failed: false,
        cancelled_failure: false,
        cancel_unwinding: false,
        last_line: 0,
        stack: None,
        stack_captures: 0,
        stack_source: "request.before".into(),
        calls: 0,
        cancel: call.cancel,
        shutdown: call.shutdown,
        deadline: call.deadline,
        request_state: call.state.snapshot(),
        request: None,
        clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system()).capture(),
        business: Default::default(),
        business_path: Default::default(),
        business_unwinding: false,
        publications: None,
        hook: None,
    }));
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    crate::protected_call::install(&lua, &state).unwrap();
    // Repeated direct `pcall` callback failures exercise the worst post-capture path.
    for count in [1, 15, 16, 17, 32] {
        let previous = state.borrow().stack_captures;
        let source =
            format!("for _=1,{count} do local ok = pcall(nil) assert(not ok) end return true");
        assert!(
            lua.load(&source)
                .set_name("request.before")
                .eval::<bool>()
                .unwrap()
        );
        assert_eq!(state.borrow().stack_captures, (previous + count).min(16));
        assert!(state.borrow().stack.is_none());
    }
    assert_eq!(state.borrow().stack_captures, 16);
    assert!(state.borrow().failed);
    assert_eq!(
        budget.failure(),
        Some(crate::limits::request::BudgetFailure::Execution)
    );
    // Cancellation releases host ownership; the next VM receives a fresh counter.
    drop(lua);
    assert_eq!(Rc::strong_count(&state), 1);
}
