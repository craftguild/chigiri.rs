//! Execute each hook in a fresh VM and return only owned results to the next phase.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use chigiri::core::{Object, Value};
use luaur::{Lua, LuaOptions, StdLib, VmState};

use super::state::{Host, Session};
use crate::cancellation::Cancellation;
use crate::codec::Codec;
use crate::hooks::{HookSlot, Rejection, RequestState};
use crate::luau::{HookScript, ScriptError};
use crate::store::Snapshot;

/// One structured log record emitted by a hook.
/// The caller supplies the surrounding request envelope and request ID.
pub struct HookLog {
    /// Closed log level: debug, info, warn, or error.
    pub level: &'static str,
    /// Validated UTF-8 message.
    pub message: String,
    /// Distinguish an object snapshot from an input-object value.
    pub fields: Object,
}

/// Nonblocking callback that only attempts to enqueue a log record.
pub type HookLogSink = Arc<dyn Fn(HookLog) + Send + Sync>;

/// Immutable phase views shared without recopying their complete values for each queued hook.
pub type HookViews = BTreeMap<&'static str, Arc<Value>>;

/// Owned inputs required to execute one hook phase.
pub struct HookCall {
    /// Store snapshot selected at phase start and shared with resolver execution.
    pub snapshot: Snapshot,
    /// Validated request-state snapshot entering this phase.
    pub state: RequestState,
    /// The same bounded request snapshot exposed to resolvers, if HTTP-backed.
    pub request: Option<Arc<Value>>,
    /// Views explicitly exposed by the shared request, operation, field, and result surface.
    pub views: HookViews,
    /// Absolute request deadline established before queue admission.
    pub deadline: Instant,
    /// Request or dropped-future cancellation signal.
    pub cancel: Cancellation,
    /// Worker-wide shutdown signal.
    pub shutdown: Cancellation,
    /// Budget shared with all other request phases.
    pub budget: super::ExecutionBudget,
    /// Nonblocking hook-log sink.
    pub log: HookLogSink,
}

/// Separate validated state candidates from rejection results so rejected hooks cannot apply patches.
pub enum HookOutcome {
    /// Continue with an optional validated state update.
    Continue(Option<RequestState>),
    /// Stop with an explicit rejection and optional invalid-rejection diagnostic.
    Rejected(Rejection, Option<ScriptError>),
}

pub(super) struct HookHost {
    pub slot: HookSlot,
    pub rejection: Option<Rejection>,
    pub reject_unwinding: bool,
    pub log: HookLogSink,
}

/// Execute a hook and release all Lua values, closures, and the VM lease before returning.
pub fn execute_hook(script: &HookScript, call: HookCall) -> Result<HookOutcome, ScriptError> {
    if !script.accepts(&call.snapshot, &call.state) {
        call.budget.0.clone().fail();
        return Err(script.error("contract", "hook schema or state contract differs"));
    }
    let deadline = call.deadline.min(Instant::now() + super::VM_TIME);
    let mut reader = call.snapshot.reader();
    reader.share_work(call.budget.0.clone());
    reader.execution_guard(&call.cancel, &call.shutdown, deadline);
    let clock = call.budget.clock();
    let state = Rc::new(RefCell::new(Host {
        ir: call.snapshot.ir().clone(),
        session: Session::Read(reader),
        failed: false,
        cancelled_failure: false,
        cancel_unwinding: false,
        last_line: 0,
        stack: None,
        stack_captures: 0,
        stack_source: script.slot().name().into(),
        calls: 0,
        cancel: call.cancel.clone(),
        shutdown: call.shutdown.clone(),
        deadline,
        request_state: call.state.snapshot(),
        request: call.request.clone(),
        clock,
        business: call.budget.2.clone(),
        business_path: call.budget.3.path.clone(),
        business_unwinding: false,
        publications: None,
        hook: Some(HookHost {
            slot: script.slot(),
            rejection: None,
            reject_unwinding: false,
            log: call.log.clone(),
        }),
    }));
    let result = super::recovery::run(
        &call.budget,
        || script.error("panic", "hook VM execution failed"),
        || run(script, &call, &state),
    );
    let mut host = Rc::try_unwrap(state)
        .map_err(|_| {
            call.budget.0.clone().fail();
            script.error("host", "hook VM ownership did not close")
        })?
        .into_inner();
    let budget = host.budget_error(script.error("budget", "hook execution cancelled or failed"));
    // Perform the final budget check without replacing a more specific error location or phase.
    let stack = host.stack.clone();
    let diagnostic = |mut error: ScriptError| {
        error.stack = stack.clone();
        error
    };
    let result = result.map_err(&diagnostic);
    let budget = budget.map_err(&diagnostic);
    if let Some(rejection) = host.hook.as_mut().and_then(|hook| hook.rejection.take()) {
        // A completed rejection cancels sibling work but must not be converted into an internal failure.
        // Hard-limit and independent failures have `cancelled = false`, so they take precedence.
        if let Err(error) = result
            && !error.cancelled
        {
            return Err(error);
        }
        if let Err(error) = budget
            && !error.cancelled
        {
            return Err(error);
        }
        if rejection.invalid {
            // Retain invalid rejection as an internal failure even if NonNull propagation drops the field result.
            call.budget.0.clone().fail();
        }
        let failure = rejection
            .invalid
            .then(|| diagnostic(ScriptError::invalid_rejection()));
        return Ok(HookOutcome::Rejected(rejection, failure));
    }
    let next = result?;
    budget?;
    call.cancel
        .commit(&call.shutdown, deadline, || HookOutcome::Continue(next))
        .ok_or_else(|| {
            let _ = host.budget();
            let mut failure = script.error("cancel", "hook cancelled before completion");
            failure.cancelled = host.cancel_unwinding;
            failure
        })
}

fn run(
    script: &HookScript,
    call: &HookCall,
    host: &Rc<RefCell<Host>>,
) -> Result<Option<RequestState>, ScriptError> {
    let error = |phase, message| {
        // Attribute VM initialization and snapshot-codec failures to the request before returning them.
        call.budget.0.clone().fail();
        script.error(phase, message)
    };
    let deadline = host.borrow().deadline;
    let _lease = crate::vm_gate::enter(deadline, || {
        call.cancel.cancelled() || call.shutdown.cancelled()
    })
    .map_err(|cause| {
        use crate::limits::request::BudgetFailure;
        use crate::vm_gate::AdmissionError;
        let mut work = call.budget.0.clone();
        match cause {
            AdmissionError::Cancelled => work.fail_with(BudgetFailure::Cancelled),
            AdmissionError::Expired => work.fail_with(BudgetFailure::Deadline),
            AdmissionError::Poisoned => work.fail_independent(),
        }
        let mut failure = script.error("budget", "hook VM admission failed");
        failure.cancelled = cause == AdmissionError::Cancelled;
        failure
    })?;
    host.borrow_mut()
        .budget_error(script.error("budget", "hook execution expired"))?;
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            call.budget
                .0
                .clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("memory", "hook VM initialization failed")
    })?;
    let codec = Rc::new(RefCell::new(
        Codec::new_with_work(&lua, call.budget.0.clone())
            .map_err(|_| error("memory", "hook codec initialization failed"))?,
    ));
    codec
        .borrow_mut()
        .share_work(host.borrow_mut().session.values().work.clone());
    let length = crate::sandbox::string_length(&lua).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            call.budget
                .0
                .clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("memory", "hook VM initialization failed")
    })?;
    let interrupt = host.clone();
    lua.set_interrupt(move |lua| {
        let mut host = interrupt.borrow_mut();
        if let Err(error) = host.budget() {
            host.capture_stack(lua, 0, "interrupt");
            return Err(error);
        }
        Ok(VmState::Continue)
    });
    let recoverable = Rc::new(std::cell::Cell::new(false));
    crate::protected_call::install_with_recoverable(&lua, host, recoverable.clone()).map_err(
        |raw| {
            if matches!(raw, luaur::Error::MemoryError(_)) {
                call.budget
                    .0
                    .clone()
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            error("host", "hook protected call initialization failed")
        },
    )?;
    let control = matches!(
        script.slot(),
        HookSlot::RequestBefore | HookSlot::OperationBefore
    );
    let time = crate::sandbox::TimeRuntime::request(host.borrow().clock.clone(), control);
    crate::sandbox::seal_with_time(&lua, call.budget.0.clone(), time.clone(), recoverable)
        .map_err(|raw| {
            if matches!(raw, luaur::Error::MemoryError(_)) {
                call.budget
                    .0
                    .clone()
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            error("memory", "hook sandbox initialization failed")
        })?;
    let run = script
        .load_observed(&lua, &length, |raw| {
            if matches!(raw, luaur::Error::MemoryError(_)) {
                call.budget
                    .0
                    .clone()
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            matches!(raw, luaur::Error::RuntimeError(_)) && host.borrow().cancel_unwinding
        })
        .inspect_err(|failure| {
            if !failure.cancelled {
                call.budget.0.clone().fail_independent();
            }
        })?;
    time.activate();
    host.borrow_mut()
        .budget_error(script.error("budget", "hook module initialization expired"))?;
    let context = super::context::context(&lua, host, &codec).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            call.budget
                .0
                .clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("host", "hook host initialization failed")
    })?;
    context.set_readonly(false);
    let fields = crate::api::hooks::phase_fields(script.slot());
    if call.views.len() != fields.len() {
        return Err(error("contract", "hook phase snapshot mismatch"));
    }
    for (name, _) in fields {
        let value = call
            .views
            .get(name)
            .ok_or_else(|| error("contract", "hook phase snapshot missing"))?;
        let value = codec
            .borrow_mut()
            .readonly(value)
            .map_err(|_| error("value", "hook snapshot conversion failed"))?;
        context
            .raw_set(name, value.as_lua().clone())
            .map_err(|raw| {
                if matches!(raw, luaur::Error::MemoryError(_)) {
                    call.budget
                        .0
                        .clone()
                        .fail_local(crate::limits::request::LocalLimit::VmMemory);
                }
                error("memory", "hook snapshot initialization failed")
            })?;
    }
    super::hook_functions::attach(&lua, host, &codec, &context).map_err(|raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            call.budget
                .0
                .clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        error("host", "hook functions initialization failed")
    })?;
    context.set_readonly(true);
    let scope = codec.borrow().scope();
    let context = scope
        .table(&context)
        .map_err(|_| error("value", "hook context conversion failed"))?;
    let mut rejected = false;
    let mut cancelled = false;
    let result = scope.call_observed(&run, &[context], |raw| {
        if matches!(raw, luaur::Error::MemoryError(_)) {
            call.budget
                .0
                .clone()
                .fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        // Classify control flow from Rust-owned interrupt state rather than matching callback text.
        // A caught `pcall` that is not one of these control paths remains an independent failure.
        rejected = matches!(raw, luaur::Error::RuntimeError(_))
            && host
                .borrow()
                .hook
                .as_ref()
                .is_some_and(|hook| hook.reject_unwinding)
            // Preserve an earlier conversion failure before recording this scope failure.
            && codec.borrow().check().is_ok();
        cancelled = matches!(raw, luaur::Error::RuntimeError(_)) && host.borrow().cancel_unwinding;
        if !rejected && !cancelled {
            // Preserve independent execution failures in the shared ledger even if a reply or sibling future is dropped.
            // Such failures take precedence over cleanup cancellation.
            call.budget.0.clone().fail_independent();
        }
    });
    if rejected {
        return Ok(None);
    }
    let result = result.map_err(|_| {
        if !cancelled {
            call.budget.0.clone().fail_independent();
        }
        let mut failure = script.error("runtime", "hook execution failed");
        failure.cancelled = cancelled;
        failure
    })?;
    host.borrow_mut()
        .budget_error(script.error("budget", "hook execution cancelled or failed"))?;
    // Ignore return values after a valid rejection; they cannot update request state.
    if host
        .borrow()
        .hook
        .as_ref()
        .is_some_and(|hook| hook.rejection.is_some())
    {
        return Ok(None);
    }
    let next = codec
        .borrow_mut()
        .hook_result(call.snapshot.ir(), script.slot(), &call.state, &result)
        .map_err(|_| {
            // Return conversion bypasses the VM error observer, so record its failure here.
            call.budget.0.clone().fail();
            error("value", "hook return conversion failed")
        })?;
    codec.borrow().check().map_err(|_| {
        call.budget.0.clone().fail();
        error("value", "hook conversion failure")
    })?;
    Ok(next)
}
