//! Validate the same closed module table before startup and after calls, without exposing `ctx` during initialization.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use luaur::{Function, Lua, LuaOptions, StdLib, Value};

use super::{Script, ScriptError};
use crate::sandbox::EvaluationFailure;

pub(super) use super::module_shape::Entrypoint;

pub(super) fn initialize(
    script: &Script,
    control: Option<&crate::validation_control::ValidationControl>,
) -> Result<(), ScriptError> {
    let deadline = control.map_or_else(
        || Instant::now() + crate::runtime::VM_TIME,
        |control| control.limit(crate::runtime::VM_TIME),
    );
    let cancellation = control.map(|c| c.cancellation.clone()).unwrap_or_default();
    let _lease = crate::vm_gate::enter(deadline, || cancellation.cancelled())
        .map_err(|_| script.error("budget", "module VM admission expired", 1, 1))?;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let error = |raw| initialization_error(script, &raw);
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(error)?;
        lua.set_memory_limit(crate::value::MAX_BYTES)
            .map_err(error)?;
        let length = crate::sandbox::string_length(&lua).map_err(error)?;
        let budget =
            crate::sandbox::install_control(&lua, deadline, cancellation).map_err(error)?;
        crate::sandbox::seal_with_evaluation(&lua, budget.clone()).map_err(error)?;
        let loaded = load_entrypoint_observed(
            &lua,
            &length,
            script,
            initialization_entrypoint(script),
            |_| false,
        );
        let final_check = budget.borrow_mut().check();
        if let Some(failure) = budget.borrow().failure() {
            return Err(evaluation_error(script, failure));
        }
        let _resolve = loaded?;
        final_check.map_err(error)?;
        // Drop Lua function callbacks inside the VM exclusion zone without invoking the resolver.
        Ok(())
    }))
    .unwrap_or_else(|_| Err(script.error("memory", "module initialization failed", 1, 1)));
    if let Some(control) = control {
        control.script()?;
    }
    result
}

#[cfg(test)]
pub(crate) fn load(lua: &Lua, length: &Function, script: &Script) -> Result<Function, ScriptError> {
    load_entrypoint_observed(lua, length, script, Entrypoint::Resolve, |_| false)
}

/// Report callback-originated cancellation before erasing the VM error variant.
pub(crate) fn load_observed(
    lua: &Lua,
    length: &Function,
    script: &Script,
    cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<Function, ScriptError> {
    load_entrypoint_observed(lua, length, script, Entrypoint::Resolve, cancelled)
}

/// Load the statically authorized subscription setup entrypoint.
pub(crate) fn load_subscription_observed(
    lua: &Lua,
    length: &Function,
    script: &Script,
    cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<Function, ScriptError> {
    load_entrypoint_observed(lua, length, script, Entrypoint::Subscribe, cancelled)
}

pub(crate) fn load_consumer_observed(
    lua: &Lua,
    length: &Function,
    script: &Script,
    cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<Function, ScriptError> {
    load_entrypoint_observed(lua, length, script, Entrypoint::Consume, cancelled)
}

fn initialization_entrypoint(script: &Script) -> Entrypoint {
    match script.module_contract() {
        super::module_shape::ModuleContract::Consumer => Entrypoint::Consume,
        super::module_shape::ModuleContract::Subscription { .. } => Entrypoint::Subscribe,
        super::module_shape::ModuleContract::Ordinary => Entrypoint::Resolve,
    }
}

/// Load one schema-authorized entrypoint without accepting a string-selected function.
pub(super) fn load_entrypoint_observed(
    lua: &Lua,
    length: &Function,
    script: &Script,
    entrypoint: Entrypoint,
    cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<Function, ScriptError> {
    load_contract_observed(lua, length, script, cancelled)?.take(entrypoint)
}

fn load_contract_observed(
    lua: &Lua,
    length: &Function,
    script: &Script,
    mut cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<super::module_shape::ResolverModule, ScriptError> {
    let value: Value = lua
        .load(script.source())
        .set_name(script.coordinate())
        .eval()
        .map_err(|raw| {
            let phase = if matches!(raw, luaur::Error::MemoryError(_)) {
                "memory"
            } else {
                "runtime"
            };
            let mut failure = script.runtime_error(phase);
            failure.cancelled = cancelled(&raw);
            failure
        })?;
    let contract = script.module_contract();
    super::module_shape::decode_resolver(value, length, contract, cancelled).map_err(|error| {
        let mut failure = script.error(error.phase, error.message, 1, 1);
        failure.cancelled = error.cancelled;
        failure
    })
}

fn initialization_error(script: &Script, raw: &luaur::Error) -> ScriptError {
    let phase = if matches!(raw, luaur::Error::MemoryError(_)) {
        "memory"
    } else {
        "runtime"
    };
    script.error(phase, "module initialization failed", 1, 1)
}

fn evaluation_error(script: &Script, failure: EvaluationFailure) -> ScriptError {
    let (phase, message) = match failure {
        EvaluationFailure::Runtime => ("runtime", "module initialization failed"),
        EvaluationFailure::Memory => ("memory", "module initialization memory limit exceeded"),
        EvaluationFailure::Reentry => ("limit", "module initialization reentry limit exceeded"),
        EvaluationFailure::Native => ("limit", "module initialization native limit exceeded"),
        EvaluationFailure::Interrupted => ("timeout", "module initialization expired"),
    };
    script.error(phase, message, 1, 1)
}

#[cfg(test)]
pub(super) fn decode(
    value: Value,
    length: &Function,
    script: &Script,
) -> Result<Function, ScriptError> {
    let contract = script.module_contract();
    super::module_shape::decode_resolver(value, length, contract, |_| false)?
        .take(Entrypoint::Resolve)
        .map_err(|error| script.error(error.phase, error.message, 1, 1))
}
