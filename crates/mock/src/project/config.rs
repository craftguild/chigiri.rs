//! Compile, type-check, and evaluate project settings, dropping all VM handles after decoding the Rust value.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use luaur::{Lua, LuaOptions, StdLib, Value};

use super::{ProjectError, ProjectSettings, config_decode};

/// Canonical configuration declarations shared by generated types and static checking.
pub const CONFIG_DECLARATIONS: &str = r#"type ChigiriStateKind =
    { kind: "string" } | { kind: "boolean" } | { kind: "integer" } | { kind: "number" }
    | { kind: "object", fields: { [string]: ChigiriStateKind } }
    | { kind: "list" | "nullable", of: ChigiriStateKind }
type ChigiriConfigValue = string | boolean | number | { [string]: ChigiriConfigValue } | { ChigiriConfigValue }
type ChigiriHookRegistration = { before: boolean, after: boolean }
type ChigiriClockConfig = { mode: "system" } | { mode: "fixed", now: number }
type ChigiriEventLimits = {
    max_queued_events: number?, max_queued_bytes: number?, max_queue_age_ms: number?,
    max_retained_bytes: number?, max_sequence_bytes: number?,
}
type ChigiriConsumerRegistration = { field: string, enabled: boolean }
type ChigiriEventSettings = {
    limits: ChigiriEventLimits?, consumers: { [string]: ChigiriConsumerRegistration }?,
}
type ChigiriMockProjectConfig = {
    format: "chigiri-mock-project/v10",
    schema: { sources: { string } },
    storage: { sources: { string } },
    clock: ChigiriClockConfig,
    server: { listen: string, allowed_origins: { string } },
    hooks: { request: ChigiriHookRegistration, operation: ChigiriHookRegistration, field: ChigiriHookRegistration },
    state: { fields: { [string]: ChigiriStateKind }, initial: { [string]: ChigiriConfigValue } },
    events: ChigiriEventSettings?,
}
"#;

/// Evaluate configuration after type-checking without exposing source or closures to the backend.
pub fn evaluate_config(source: &str) -> Result<ProjectSettings, ProjectError> {
    check(source, CONFIG_DECLARATIONS)?;
    let deadline = Instant::now() + crate::runtime::VM_TIME;
    let _lease = crate::vm_gate::enter(deadline, || false)
        .map_err(|_| ProjectError::new("budget", "config VM admission expired"))?;
    // Convert a panic during pinned VM allocation into an error before any output escapes.
    catch_unwind(AssertUnwindSafe(|| {
        evaluate(source, deadline, super::FORMAT)
    }))
    .unwrap_or_else(|_| Err(ProjectError::new("memory", "config VM allocation failed")))
}

fn check(source: &str, declarations: &str) -> Result<(), ProjectError> {
    if source.len() > crate::limits::CONFIG_BYTES {
        return Err(ProjectError::new("limit", "config source exceeds 256 KiB"));
    }
    let _lease = crate::vm_gate::enter(Instant::now() + Duration::from_secs(5), || false)
        .map_err(|_| ProjectError::new("environment", "type checker admission expired"))?;
    // Validate standalone syntax before checking the expected return wrapper.
    luaur::compile(source)
        .map_err(|_| ProjectError::new("compile", "config syntax or compilation failed"))?;
    let capability_declarations = crate::luau::capability_declarations()
        .map_err(|_| ProjectError::new("contract", "capability declarations failed"))?;
    let prefix = format!(
        "{}{capability_declarations}{declarations}local function project(): ChigiriMockProjectConfig\n",
        crate::luau::checker_prelude()
    );
    let wrapped = format!("{prefix}{source}\nend\nreturn project()\n");
    if wrapped.len() > crate::luau::MAX_WRAPPER {
        return Err(ProjectError::new("limit", "config type wrapper byte limit"));
    }
    luaur::check_with_definitions(&wrapped, "").map_err(|errors| {
        let mut error = ProjectError::new("type", "config does not match the project contract");
        if let Some(first) = errors.iter().find(|item| !item.in_definitions) {
            error.set_position((
                (first.line as usize)
                    .saturating_sub(prefix.lines().count())
                    .max(1),
                first.column as usize,
            ));
        }
        error
    })
}

fn evaluate(
    source: &str,
    deadline: Instant,
    expected_format: &str,
) -> Result<ProjectSettings, ProjectError> {
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(vm_error)?;
    lua.set_memory_limit(crate::value::MAX_BYTES)
        .map_err(vm_error)?;
    let length = crate::sandbox::string_length(&lua).map_err(vm_error)?;
    let state = crate::sandbox::install_budget(&lua, deadline).map_err(vm_error)?;
    crate::sandbox::seal_with_evaluation(&lua, state.clone()).map_err(vm_error)?;
    let value = lua
        .load(source)
        .set_name("mock.luau")
        .eval::<Value>()
        .map_err(vm_error)?;
    state.borrow_mut().check().map_err(vm_error)?;
    let result = config_decode::decode(&lua, value, length, expected_format)?;
    state.borrow_mut().check().map_err(vm_error)?;
    Ok(result)
}

fn vm_error(_: luaur::Error) -> ProjectError {
    ProjectError::new("runtime", "config evaluation failed")
}
