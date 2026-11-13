//! Execute with a fresh VM and codec, exposing only the permitted global surface.

use std::cell::RefCell;
use std::rc::Rc;

use chigiri::core::{Object, Value};
use luaur::{Lua, LuaOptions, StdLib, Value as LuaValue, VmState};

use super::state::Host;
use crate::codec::Codec;
use crate::codec::Provenance;
use crate::luau::{Script, ScriptError};
use crate::output::{OutputObject, OutputValue};

mod invocation;
use crate::protected_call::State;

use super::subscription_setup::{BindingCollector, SubscriptionBinding};

/// Observable sandbox properties returned without retaining a VM handle.
pub struct EnvironmentInfo {
    /// Allowed global keys present in the VM.
    pub globals: Vec<String>,
    /// String method lookup exposes the same closed table as `string`.
    pub string_methods_restricted: bool,
}

pub(super) struct VmOutput {
    pub value: Value,
    pub provenance: Provenance,
}

#[derive(Clone, Copy)]
enum CallKind<'a> {
    Resolve(Option<&'a OutputObject>),
    Subscribe {
        bindings: &'a BindingCollector,
        log: Option<&'a super::HookLogSink>,
    },
    Consume(&'a ConsumerInvocation<'a>),
}

enum CallOutput {
    Resolve(VmOutput),
    Subscribe(SubscriptionBinding),
    Consume,
}

pub(super) struct ConsumerInvocation<'a> {
    pub event: &'a OutputValue,
    pub metadata: &'a Object,
    pub log: &'a super::HookLogSink,
}

/// Execute a closure after installing the selected allowlist into a fresh Lua VM.
pub fn inspect_environment() -> luaur::Result<EnvironmentInfo> {
    let _lease = crate::vm_gate::enter(std::time::Instant::now() + super::VM_TIME, || false)
        .map_err(|_| luaur::Error::runtime("VM admission expired"))?;
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())?;
    lua.set_memory_limit(crate::value::MAX_BYTES)?;
    crate::sandbox::seal(&lua)?;
    let globals = lua
        .globals()
        .pairs::<String, LuaValue>()
        .map(|entry| entry.map(|(key, _)| key))
        .collect::<luaur::Result<Vec<_>>>()?;
    let string_methods_restricted = lua
        .load("return ('abc'):sub(2) == 'bc' and ('x').rep == nil")
        .eval::<bool>()?;
    Ok(EnvironmentInfo {
        globals,
        string_methods_restricted,
    })
}

#[cfg(test)]
pub(super) fn run(
    script: &Script,
    args: &Object,
    parent: Option<&OutputObject>,
    state: &Rc<RefCell<Host>>,
) -> Result<Value, ScriptError> {
    run_with(script, args, parent, state, || {})
}

pub(super) fn run_output(
    script: &Script,
    args: &Object,
    parent: Option<&OutputObject>,
    state: &Rc<RefCell<Host>>,
) -> Result<VmOutput, ScriptError> {
    run_output_with(script, args, parent, state, || {})
}

pub(super) fn run_subscription(
    script: &Script,
    args: &Object,
    state: &Rc<RefCell<Host>>,
    bindings: &BindingCollector,
    log: Option<&super::HookLogSink>,
) -> Result<SubscriptionBinding, ScriptError> {
    match run_call(
        script,
        args,
        state,
        CallKind::Subscribe { bindings, log },
        || {},
    )? {
        CallOutput::Subscribe(binding) => Ok(binding),
        _ => Err(script.error("invariant", "setup result mismatch", 1, 1)),
    }
}

pub(super) fn run_consumer(
    script: &Script,
    state: &Rc<RefCell<Host>>,
    call: &ConsumerInvocation<'_>,
) -> Result<(), ScriptError> {
    match run_call(
        script,
        &Object::new(),
        state,
        CallKind::Consume(call),
        || {},
    )? {
        CallOutput::Consume => Ok(()),
        _ => Err(script.error("invariant", "consumer result mismatch", 1, 1)),
    }
}

#[cfg(test)]
pub(super) fn run_with(
    script: &Script,
    args: &Object,
    parent: Option<&OutputObject>,
    state: &Rc<RefCell<Host>>,
    after_initialization: impl FnOnce(),
) -> Result<Value, ScriptError> {
    run_output_with(script, args, parent, state, after_initialization).map(|output| output.value)
}

fn run_output_with(
    script: &Script,
    args: &Object,
    parent: Option<&OutputObject>,
    state: &Rc<RefCell<Host>>,
    after_initialization: impl FnOnce(),
) -> Result<VmOutput, ScriptError> {
    match run_call(
        script,
        args,
        state,
        CallKind::Resolve(parent),
        after_initialization,
    )? {
        CallOutput::Resolve(output) => Ok(output),
        _ => Err(script.error("invariant", "resolver result mismatch", 1, 1)),
    }
}

fn run_call(
    script: &Script,
    args: &Object,
    state: &Rc<RefCell<Host>>,
    kind: CallKind<'_>,
    after_initialization: impl FnOnce(),
) -> Result<CallOutput, ScriptError> {
    let work = state.borrow_mut().session.values().work.clone();
    let error = |phase, message| {
        work.clone().fail();
        script.error(phase, message, 1, 1)
    };
    let deadline = state.borrow().deadline;
    let _lease = crate::vm_gate::enter(deadline, || {
        let host = state.borrow();
        host.cancel.cancelled() || host.shutdown.cancelled()
    })
    .map_err(|cause| {
        use crate::limits::request::BudgetFailure;
        use crate::vm_gate::AdmissionError;
        match cause {
            AdmissionError::Cancelled => work.clone().fail_with(BudgetFailure::Cancelled),
            AdmissionError::Expired => work.clone().fail_with(BudgetFailure::Deadline),
            AdmissionError::Poisoned => work.clone().fail_independent(),
        }
        let mut failure = script.error("budget", "VM admission failed", 1, 1);
        failure.cancelled = cause == AdmissionError::Cancelled;
        failure
    })?;
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("memory", "VM initialization failed")
    })?;
    let codec = Rc::new(RefCell::new(
        Codec::new_with_work(&lua, work.clone())
            .map_err(|_| error("memory", "VM initialization failed"))?,
    ));
    codec
        .borrow_mut()
        .share_work(state.borrow_mut().session.values().work.clone());
    let length = crate::sandbox::string_length(&lua).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("memory", "VM initialization failed")
    })?;
    let interrupt = state.clone();
    lua.set_interrupt(move |lua| {
        let mut host = interrupt.borrow_mut();
        host.observe_line(lua, 0);
        if let Err(error) = host.budget() {
            host.capture_stack(lua, 0, "interrupt");
            return Err(error);
        }
        Ok(VmState::Continue)
    });
    let recoverable = Rc::new(std::cell::Cell::new(false));
    crate::protected_call::install_with_recoverable(&lua, state, recoverable.clone()).map_err(
        |raw| {
            if matches!(raw, luaur::Error::MemoryError(_)) {
                work.clone()
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            error("host", "protected call initialization failed")
        },
    )?;
    let time = crate::sandbox::TimeRuntime::request(state.borrow().clock.clone(), false);
    crate::sandbox::seal_with_time(&lua, work.clone(), time.clone(), recoverable).map_err(
        |raw| {
            if matches!(raw, luaur::Error::MemoryError(_)) {
                work.clone()
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            error("memory", "VM initialization failed")
        },
    )?;
    let cancelled = |raw: &luaur::Error| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        matches!(raw, luaur::Error::RuntimeError(_)) && state.borrow().cancel_unwinding
    };
    let resolve = match kind {
        CallKind::Resolve(_) => crate::luau::load_module_observed(&lua, &length, script, cancelled),
        CallKind::Subscribe { .. } => {
            crate::luau::load_subscription_observed(&lua, &length, script, cancelled)
        }
        CallKind::Consume(_) => {
            crate::luau::load_consumer_observed(&lua, &length, script, cancelled)
        }
    }
    .inspect_err(|failure| {
        if !failure.cancelled {
            work.clone().fail_independent();
        }
    })?;
    time.activate();
    after_initialization();
    state.borrow_mut().budget_error(script.error(
        "budget",
        "module initialization expired",
        1,
        1,
    ))?;
    let context = invocation::context(&lua, state, &codec, kind).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("host", "host initialization failed")
    })?;
    let scope = codec.borrow().scope();
    let arguments = invocation::arguments(&mut codec.borrow_mut(), &context, args, kind)
        .map_err(|_| error("value", "argument conversion failed"))?;
    let mut diagnostic = None;
    let observe = |raw: &luaur::Error| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            work.clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        // Charge only otherwise-untracked deallocation to the shared ledger.
        let host = state.borrow();
        let cancelled = matches!(raw, luaur::Error::RuntimeError(_)) && host.cancel_unwinding;
        let business =
            matches!(raw, luaur::Error::RuntimeError(_)) && host.business_unwinding && !host.failed;
        drop(host);
        if !cancelled && !business {
            work.clone().fail_independent();
        }
        let phase = if business {
            "business"
        } else if matches!(raw, luaur::Error::MemoryError(_)) {
            "memory"
        } else {
            "runtime"
        };
        let mut failure = if business {
            script.error(phase, "business rejection", state.borrow().last_line, 1)
        } else {
            script.runtime_error(phase)
        };
        failure.cancelled = cancelled;
        diagnostic = Some(failure);
    };
    let result = if matches!(kind, CallKind::Consume(_)) {
        scope.call_void_observed(&resolve, &arguments, observe)
    } else {
        scope.call_observed(&resolve, &arguments, observe)
    }
    .map_err(|_| diagnostic.unwrap_or_else(|| error("value", "VM execution failed")))?;
    let output = invocation::output(script, &mut codec.borrow_mut(), &result, kind)
        .inspect_err(|_| work.clone().fail())?;
    codec
        .borrow()
        .check()
        .map_err(|_| error("value", "conversion failure prevents commit"))?;
    Ok(output)
}
