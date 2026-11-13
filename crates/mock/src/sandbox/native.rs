//! Bounded wrappers around allowed VM-native operations.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use luaur::{Error, Function, Lua, LuaString, MultiValue, Value};

const PATTERN_BYTES: usize = 256;

#[derive(Clone)]
pub(super) struct Budget {
    used: Rc<Cell<usize>>,
    work: Option<crate::value::work::Work>,
    startup: Option<Rc<RefCell<super::Evaluation>>>,
    recoverable: Option<Rc<Cell<bool>>>,
}

impl Budget {
    pub(super) fn isolated() -> Self {
        Self {
            used: Rc::new(Cell::new(0)),
            work: None,
            startup: None,
            recoverable: None,
        }
    }

    pub(super) fn request(work: crate::value::work::Work, recoverable: Rc<Cell<bool>>) -> Self {
        Self {
            used: Rc::new(Cell::new(0)),
            work: Some(work),
            startup: None,
            recoverable: Some(recoverable),
        }
    }

    pub(super) fn startup(state: Rc<RefCell<super::Evaluation>>) -> Self {
        Self {
            used: Rc::new(Cell::new(0)),
            work: None,
            startup: Some(state),
            recoverable: None,
        }
    }

    pub(super) fn charge(&self, count: usize) -> luaur::Result<()> {
        if let Some(work) = &self.work {
            let mut work = work.clone();
            return work.charge(count).map_err(|_| limit());
        }
        let observed = self.used.get().saturating_add(count);
        if observed > crate::api::WORK_LIMIT {
            return self.reject();
        }
        self.used.set(observed);
        Ok(())
    }

    pub(super) fn reject<T>(&self) -> luaur::Result<T> {
        if let Some(work) = &self.work {
            let mut work = work.clone();
            let _ = work.charge(usize::MAX);
        }
        if let Some(state) = &self.startup {
            state.borrow_mut().native_limit();
        }
        Err(limit())
    }

    pub(super) fn runtime_failure(&self) {
        if let Some(work) = &self.work {
            let mut work = work.clone();
            work.fail_independent();
        }
        if let Some(state) = &self.startup {
            state.borrow_mut().record(super::EvaluationFailure::Runtime);
        }
    }

    pub(super) fn memory_failure(&self) {
        if let Some(work) = &self.work {
            let mut work = work.clone();
            work.fail_local(crate::limits::request::LocalLimit::VmMemory);
        }
        if let Some(state) = &self.startup {
            state.borrow_mut().record(super::EvaluationFailure::Memory);
        }
    }

    pub(super) fn preserve_boundary_error(&self, error: Error) -> Error {
        if matches!(&error, Error::MemoryError(_)) {
            self.memory_failure();
        }
        error
    }

    pub(super) fn recoverable_native_error(&self) {
        if let Some(recoverable) = &self.recoverable {
            recoverable.set(true);
        }
    }
}

pub(super) fn guard(
    lua: &Lua,
    library: Option<&'static str>,
    name: &'static str,
    original: Function,
    budget: &Budget,
) -> luaur::Result<Value> {
    let key = format!("mock.native.{}.{name}", library.unwrap_or("global"));
    lua.set_named_registry_value(&key, original)?;
    let sorting = Cell::new(false);
    let budget = budget.clone();
    Ok(Value::Function(lua.create_function(
        move |lua, arguments: MultiValue| {
            let cost = validate(library, name, &arguments).unwrap_or(usize::MAX);
            if cost == usize::MAX {
                return budget.reject();
            }
            budget.charge(cost)?;
            let result = if library == Some("table") && name == "sort" {
                if sorting.replace(true) {
                    return budget.reject();
                }
                let result = call(lua, &key, arguments, &budget);
                sorting.set(false);
                result
            } else {
                call(lua, &key, arguments, &budget)
            };
            if library.is_none() && name == "assert" {
                let mut values = result?;
                let value = values
                    .pop_front()
                    .ok_or_else(|| Error::runtime("assertion result missing"))?;
                Ok(MultiValue::from_vec(vec![value]))
            } else if library == Some("string") && name == "find" {
                Ok(MultiValue::from_vec(result?.into_iter().take(2).collect()))
            } else {
                result
            }
        },
    )?))
}

fn validate(library: Option<&str>, name: &str, arguments: &MultiValue) -> luaur::Result<usize> {
    match library {
        None if name == "assert" => {
            if arguments.is_empty()
                || arguments.len() > 2
                || arguments
                    .get(1)
                    .is_some_and(|value| !matches!(value, Value::String(_) | Value::Nil))
            {
                return Err(limit());
            }
            Ok(1)
        }
        None if name == "tonumber" => string_at(arguments, 0, crate::value::MAX_STRING_BYTES),
        Some("string") => {
            let mut cost = string_at(arguments, 0, crate::value::MAX_STRING_BYTES)?;
            if name == "find" {
                cost = cost.saturating_add(string_at(arguments, 1, PATTERN_BYTES)?);
            }
            Ok(cost.max(1))
        }
        Some("table") => table(arguments, name),
        _ => Ok(1),
    }
}

fn string_at(arguments: &MultiValue, index: usize, maximum: usize) -> luaur::Result<usize> {
    if let Some(Value::String(value)) = arguments.get(index) {
        let bytes = string_bytes(value);
        if bytes > maximum {
            return Err(limit());
        }
        return Ok(bytes.max(1));
    }
    Ok(1)
}

fn string_bytes(value: &LuaString) -> usize {
    value.as_bytes().len()
}

fn table(arguments: &MultiValue, name: &str) -> luaur::Result<usize> {
    let Some(Value::Table(table)) = arguments.front() else {
        return Ok(1);
    };
    let maximum = crate::value::MAX_ITEMS;
    if name == "clone" {
        let mut count = 0;
        for entry in table.clone().pairs::<Value, Value>() {
            entry?;
            count += 1;
            if count > maximum {
                return Err(limit());
            }
        }
        return Ok(count.max(1));
    } else if table.raw_len() > maximum || name == "insert" && table.raw_len() == maximum {
        return Err(limit());
    }
    Ok(table.raw_len().max(1))
}

fn call(lua: &Lua, key: &str, arguments: MultiValue, budget: &Budget) -> luaur::Result<MultiValue> {
    let original: Function = lua.named_registry_value(key)?;
    original
        .call(arguments)
        .map_err(|error| budget.preserve_boundary_error(error))
}

fn limit() -> Error {
    Error::runtime("native capability limit exceeded")
}
