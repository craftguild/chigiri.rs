//! Apply the same hook-module validation at startup and runtime without invoking `run` during initialization.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use luaur::{Function, Lua, LuaOptions, StdLib, Value};

use crate::validation_control::ValidationControl;

use super::{HookScript, ScriptError};
use crate::sandbox::EvaluationFailure;

pub(super) fn initialize(
    script: &HookScript,
    control: Option<&ValidationControl>,
) -> Result<(), ScriptError> {
    let deadline = control.map_or_else(
        || Instant::now() + crate::runtime::VM_TIME,
        |c| c.limit(crate::runtime::VM_TIME),
    );
    let cancellation = control.map(|c| c.cancellation.clone()).unwrap_or_default();
    let _lease = crate::vm_gate::enter(deadline, || cancellation.cancelled())
        .map_err(|_| script.error("budget", "hook VM admission expired"))?;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let invalid = |raw| initialization_error(script, &raw);
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(invalid)?;
        lua.set_memory_limit(crate::value::MAX_BYTES)
            .map_err(invalid)?;
        let length = crate::sandbox::string_length(&lua).map_err(invalid)?;
        let budget =
            crate::sandbox::install_control(&lua, deadline, cancellation).map_err(invalid)?;
        crate::sandbox::seal_with_evaluation(&lua, budget.clone()).map_err(invalid)?;
        let loaded = script.load(&lua, &length);
        let final_check = budget.borrow_mut().check();
        if let Some(failure) = budget.borrow().failure() {
            return Err(evaluation_error(script, failure));
        }
        let _run = loaded?;
        final_check.map_err(invalid)?;
        Ok(())
    }))
    .unwrap_or_else(|_| Err(script.error("memory", "hook initialization failed")));
    if let Some(control) = control {
        control.script()?;
    }
    result
}

impl HookScript {
    /// Load and validate the closed `run` module for this exact hook slot.
    pub fn load(&self, lua: &Lua, length: &Function) -> Result<Function, ScriptError> {
        self.load_observed(lua, length, |_| false)
    }

    /// Classify cancellation while the raw initialization error is still available.
    pub(crate) fn load_observed(
        &self,
        lua: &Lua,
        length: &Function,
        mut cancelled: impl FnMut(&luaur::Error) -> bool,
    ) -> Result<Function, ScriptError> {
        let value: Value = lua
            .load(self.source())
            .set_name(self.slot().name())
            .eval()
            .map_err(|raw| {
                let phase = if matches!(raw, luaur::Error::MemoryError(_)) {
                    "memory"
                } else {
                    "runtime"
                };
                let mut error = self.error(phase, "hook initialization failed");
                error.cancelled = cancelled(&raw);
                error
            })?;
        super::module_shape::decode(value, length, crate::api::hooks::RUN, cancelled).map_err(
            |error| {
                let mut failure = self.error(error.phase, error.message);
                failure.cancelled = error.cancelled;
                failure
            },
        )
    }
}

fn initialization_error(script: &HookScript, raw: &luaur::Error) -> ScriptError {
    let phase = if matches!(raw, luaur::Error::MemoryError(_)) {
        "memory"
    } else {
        "runtime"
    };
    script.error(phase, "hook initialization failed")
}

fn evaluation_error(script: &HookScript, failure: EvaluationFailure) -> ScriptError {
    let (phase, message) = match failure {
        EvaluationFailure::Runtime => ("runtime", "hook initialization failed"),
        EvaluationFailure::Memory => ("memory", "hook initialization memory limit exceeded"),
        EvaluationFailure::Reentry => ("limit", "hook initialization reentry limit exceeded"),
        EvaluationFailure::Native => ("limit", "hook initialization native limit exceeded"),
        EvaluationFailure::Interrupted => ("timeout", "hook initialization expired"),
    };
    script.error(phase, message)
}
