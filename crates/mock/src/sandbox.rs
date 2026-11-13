//! Minimal global boundary shared by setup, initialization, hooks, and resolvers.

mod native;
mod time;
mod time_api;

pub(crate) use time::Runtime as TimeRuntime;

use luaur::{Lua, LuaString, Table, Value};

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Sticky deadline and failure state shared during pre-boot module initialization.
pub(crate) struct Evaluation {
    deadline: Instant,
    cancellation: crate::cancellation::Cancellation,
    reject_caught_errors: bool,
    failure: Option<EvaluationFailure>,
}

/// Retained cause that makes a pre-boot VM result unusable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvaluationFailure {
    Runtime,
    Memory,
    Reentry,
    Native,
    Interrupted,
}

impl Evaluation {
    pub(crate) fn check(&mut self) -> luaur::Result<()> {
        if self.cancellation.cancelled() || Instant::now() >= self.deadline {
            self.record(EvaluationFailure::Interrupted);
            Err(luaur::Error::runtime("evaluation cancelled or expired"))
        } else if self.failure.is_some() {
            Err(luaur::Error::runtime("evaluation failed"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn failure(&self) -> Option<EvaluationFailure> {
        self.failure
    }

    fn record(&mut self, failure: EvaluationFailure) {
        if self.failure.is_none() || self.failure == Some(EvaluationFailure::Runtime) {
            self.failure = Some(failure);
        }
    }

    fn native_limit(&mut self) {
        self.record(EvaluationFailure::Native);
    }
}

impl crate::protected_call::State for Evaluation {
    fn observe_line(&mut self, _: &Lua, _: usize) {}
    fn fail(&mut self) {
        self.record(EvaluationFailure::Runtime);
    }
    fn reentry_limit(&mut self) {
        self.record(EvaluationFailure::Reentry);
    }
    fn caught_error(&mut self, error: &luaur::Error) {
        if matches!(error, luaur::Error::MemoryError(_)) {
            self.record(EvaluationFailure::Memory);
        } else if self.reject_caught_errors {
            self.record(EvaluationFailure::Runtime);
        }
    }
}

pub(crate) fn install_budget(
    lua: &Lua,
    deadline: Instant,
) -> luaur::Result<Rc<RefCell<Evaluation>>> {
    install_control(lua, deadline, crate::cancellation::Cancellation::default())
}

pub(crate) fn install_control(
    lua: &Lua,
    deadline: Instant,
    cancellation: crate::cancellation::Cancellation,
) -> luaur::Result<Rc<RefCell<Evaluation>>> {
    install_control_mode(lua, deadline, cancellation, false)
}

pub(crate) fn install_sticky_control(
    lua: &Lua,
    deadline: Instant,
    cancellation: crate::cancellation::Cancellation,
) -> luaur::Result<Rc<RefCell<Evaluation>>> {
    install_control_mode(lua, deadline, cancellation, true)
}

fn install_control_mode(
    lua: &Lua,
    deadline: Instant,
    cancellation: crate::cancellation::Cancellation,
    reject_caught_errors: bool,
) -> luaur::Result<Rc<RefCell<Evaluation>>> {
    let state = Rc::new(RefCell::new(Evaluation {
        deadline,
        cancellation,
        reject_caught_errors,
        failure: None,
    }));
    let interrupt = state.clone();
    lua.set_interrupt(move |_| {
        interrupt.borrow_mut().check()?;
        Ok(luaur::VmState::Continue)
    });
    crate::protected_call::install(lua, &state)?;
    Ok(state)
}

/// Return the pinned Luau string-length function used to measure before copying bytes into Rust.
pub(crate) fn string_length(lua: &Lua) -> luaur::Result<luaur::Function> {
    let strings: luaur::Table = lua.globals().raw_get("string")?;
    strings.raw_get("len")
}

pub(crate) fn seal(lua: &Lua) -> luaur::Result<()> {
    seal_with_budget(
        lua,
        native::Budget::isolated(),
        TimeRuntime::initialization(),
    )
}

pub(crate) fn seal_with_time(
    lua: &Lua,
    work: crate::value::work::Work,
    time: std::sync::Arc<TimeRuntime>,
    recoverable: Rc<std::cell::Cell<bool>>,
) -> luaur::Result<()> {
    seal_with_budget(lua, native::Budget::request(work, recoverable), time)
}

pub(crate) fn seal_with_evaluation(lua: &Lua, state: Rc<RefCell<Evaluation>>) -> luaur::Result<()> {
    seal_with_budget(
        lua,
        native::Budget::startup(state),
        TimeRuntime::initialization(),
    )
}

pub(crate) fn seal_with_time_evaluation(
    lua: &Lua,
    state: Rc<RefCell<Evaluation>>,
    time: std::sync::Arc<TimeRuntime>,
) -> luaur::Result<()> {
    seal_with_budget(lua, native::Budget::startup(state), time)
}

fn seal_with_budget(
    lua: &Lua,
    budget: native::Budget,
    time: std::sync::Arc<TimeRuntime>,
) -> luaur::Result<()> {
    let globals = lua.create_table_result()?;
    for function in crate::api::runtime::GLOBALS {
        globals.raw_set(
            function.name,
            native::guard(
                lua,
                None,
                function.name,
                lua.globals().raw_get(function.name)?,
                &budget,
            )?,
        )?;
    }
    let mut string_library = None;
    for library in crate::api::runtime::LIBRARIES {
        if library.name == "timeleap" {
            globals.raw_set(
                library.name,
                time_api::timeleap_library(lua, &budget, &time)?,
            )?;
            continue;
        }
        let original: Table = lua.globals().raw_get(library.name)?;
        if library.name == "os" {
            globals.raw_set(
                library.name,
                time_api::os_library(lua, &original, &budget, &time)?,
            )?;
            continue;
        }
        let selected = lua.create_table_result()?;
        for function in library.functions {
            selected.raw_set(
                function.name,
                native::guard(
                    lua,
                    Some(library.name),
                    function.name,
                    original.raw_get(function.name)?,
                    &budget,
                )?,
            )?;
        }
        selected.set_readonly(true);
        if library.name == "string" {
            string_library = Some(selected.clone());
        }
        globals.raw_set(library.name, selected)?;
    }
    let metatable = lua.create_table_result()?;
    metatable.raw_set(
        "__index",
        string_library.ok_or_else(|| luaur::Error::runtime("string capability missing"))?,
    )?;
    metatable.set_readonly(true);
    lua.set_type_metatable::<LuaString>(Some(metatable));
    for name in crate::api::runtime::UNAVAILABLE_GLOBALS {
        globals.raw_set(*name, Value::Nil)?;
    }
    globals.raw_set("_G", globals.clone())?;
    lua.set_globals(globals)
}
