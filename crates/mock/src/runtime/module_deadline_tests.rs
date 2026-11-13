//! Verify scheduler delay during initialization cannot renew a resolver's fixed VM deadline.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use super::state::{Host, Session};
use super::tests::contract;
use crate::cancellation::Cancellation;
use crate::ir::ScriptId;
use crate::luau::{Script, ScriptCandidate};
use crate::store::Store;

fn host(script: &Script) -> Rc<RefCell<Host>> {
    Rc::new(RefCell::new(Host {
        ir: script.ir().clone(),
        session: Session::Read(Store::empty(script.ir().clone()).snapshot().reader()),
        failed: false,
        cancelled_failure: false,
        cancel_unwinding: false,
        last_line: 0,
        stack: None,
        stack_captures: 0,
        stack_source: script.coordinate().into(),
        calls: 0,
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        deadline: Instant::now() + super::VM_TIME,
        request_state: Arc::new(Object::new()),
        request: None,
        clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system()).capture(),
        business: Default::default(),
        business_path: Default::default(),
        business_unwinding: false,
        publications: None,
        hook: None,
    }))
}

#[test]
fn mp_22_bounded_initialization_and_resolve_share_original_deadline_even_after_descheduling() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value:Int! }");
    let mut iterations = 10_000;
    let script = loop {
        let source = format!(
            r#"
local function work(): number
    local total = 0
    for i = 1, {iterations} do total += i end
    return total
end
local initialized = work()
assert(initialized > 0)
return {{resolve=function(): number
    assert(work() == initialized)
    return 42
end}}
"#
        );
        let script = contract
            .script(ScriptCandidate {
                script: ScriptId(0),
                source,
            })
            .unwrap();
        let start = Instant::now();
        assert!(matches!(
            super::vm::run(&script, &Object::new(), None, &host(&script)).unwrap(),
            Value::Int(42)
        ));
        // Use the same bounded work in both cases instead of a machine-dependent loop count.
        if start.elapsed() >= Duration::from_millis(120) {
            break script;
        }
        iterations *= 2;
        assert!(
            iterations <= 100_000_000,
            "finite workload calibration failed"
        );
    };
    let state = host(&script);
    let deadline = state.borrow().deadline;
    let mut initialized = false;
    let result = super::vm::run_with(&script, &Object::new(), None, &state, || {
        initialized = true;
        assert_eq!(state.borrow().deadline, deadline);
        let resume = deadline - Duration::from_millis(20);
        if let Some(wait) = resume.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    });
    assert!(initialized);
    assert!(result.is_err());
    assert_eq!(state.borrow().deadline, deadline);
    assert!(state.borrow().failed);
    // The same source succeeds in a fresh VM, proving no VM or closure leaked from initialization.
    assert!(matches!(
        super::vm::run(&script, &Object::new(), None, &host(&script)).unwrap(),
        Value::Int(42)
    ));
}

#[test]
fn module_initialization_reports_cancellation_and_deadline_as_distinct_causes() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value:Int! }");
    let script = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source: "return {resolve=function(): number return 42 end}".into(),
        })
        .unwrap();
    for cancelled in [true, false] {
        let state = host(&script);
        let mut initialized = false;
        let failure = super::vm::run_with(&script, &Object::new(), None, &state, || {
            initialized = true;
            if cancelled {
                state.borrow().cancel.cancel();
            } else {
                state.borrow_mut().deadline = Instant::now() - Duration::from_millis(1);
            }
        })
        .unwrap_err();
        assert!(initialized);
        assert_eq!(failure.cancelled, cancelled);
        let mut host = state.borrow_mut();
        assert!(host.failed);
        assert_eq!(
            host.session.values().work.failure(),
            Some(if cancelled {
                crate::limits::request::BudgetFailure::Cancelled
            } else {
                crate::limits::request::BudgetFailure::Deadline
            })
        );
    }
    // A fresh VM can run the same script after the failed VM is discarded.
    assert!(matches!(
        super::vm::run(&script, &Object::new(), None, &host(&script)).unwrap(),
        Value::Int(42)
    ));
}

#[test]
fn module_evaluation_cancellation_preserves_local_causes_except_pure_vm_errors() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value:Int! }");
    let script = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source: "local n=0 for i=1,10000 do n+=i end return {resolve=function(): number return n end}".into(),
        })
        .unwrap();
    let state = host(&script);
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let length = crate::sandbox::string_length(&lua).unwrap();
    let interrupt = state.clone();
    lua.set_interrupt(move |_| {
        let mut host = interrupt.borrow_mut();
        host.cancel.cancel();
        host.budget()?;
        Ok(luaur::VmState::Continue)
    });
    let failure = crate::luau::load_module_observed(&lua, &length, &script, |raw| {
        matches!(raw, luaur::Error::RuntimeError(_)) && state.borrow().cancel_unwinding
    })
    .unwrap_err();
    assert!(failure.cancelled);
    let mut host = state.borrow_mut();
    assert!(host.failed);
    assert_eq!(
        host.session.values().work.failure(),
        Some(crate::limits::request::BudgetFailure::Cancelled)
    );
}
