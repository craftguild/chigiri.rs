//! Bounded wrappers for the selected native `os` surface and `timeleap`.

use std::sync::Arc;

use luaur::{Error, Function, Lua, MultiValue, Table, Value};

use super::native::Budget;
use super::time::Runtime;

const DATE_RESULT_FIELDS: usize = 9;

pub(super) fn os_library(
    lua: &Lua,
    original: &Table,
    budget: &Budget,
    runtime: &Arc<Runtime>,
) -> luaur::Result<Table> {
    for name in ["time", "date", "difftime", "clock"] {
        let function: Function = original.raw_get(name)?;
        lua.set_named_registry_value(&format!("mock.native.os.{name}"), function)?;
    }
    let selected = lua.create_table_result()?;
    selected.raw_set("time", time(lua, budget, runtime)?)?;
    selected.raw_set("date", date(lua, budget, runtime)?)?;
    selected.raw_set(
        "difftime",
        passthrough(lua, "difftime", budget, runtime, false)?,
    )?;
    selected.raw_set("clock", passthrough(lua, "clock", budget, runtime, true)?)?;
    selected.set_readonly(true);
    Ok(selected)
}

pub(super) fn timeleap_library(
    lua: &Lua,
    budget: &Budget,
    runtime: &Arc<Runtime>,
) -> luaur::Result<Table> {
    let selected = lua.create_table_result()?;
    let state = runtime.clone();
    let limit = budget.clone();
    selected.raw_set(
        "travel_to",
        lua.create_function(move |lua, arguments: MultiValue| {
            limit.charge(1)?;
            if !(1..=2).contains(&arguments.len()) {
                return Err(fail(&limit));
            }
            let timestamp = timestamp(arguments.front()).ok_or_else(|| fail(&limit))?;
            let world = options(arguments.get(1)).map_err(|_| fail(&limit))?;
            state
                .travel_to(lua, timestamp, world)
                .map_err(|_| fail(&limit))
        })?,
    )?;
    let state = runtime.clone();
    let limit = budget.clone();
    selected.raw_set(
        "reset",
        lua.create_function(move |_, arguments: MultiValue| {
            limit.charge(1)?;
            if arguments.len() > 1 {
                return Err(fail(&limit));
            }
            let world = options(arguments.front()).map_err(|_| fail(&limit))?;
            state.reset(world).map_err(|_| fail(&limit))
        })?,
    )?;
    selected.set_readonly(true);
    Ok(selected)
}

fn time(lua: &Lua, budget: &Budget, runtime: &Arc<Runtime>) -> luaur::Result<Function> {
    let state = runtime.clone();
    let limit = budget.clone();
    lua.create_function(move |lua, arguments: MultiValue| {
        limit.charge(1)?;
        if arguments
            .front()
            .is_none_or(|value| matches!(value, Value::Nil))
        {
            return state
                .now()
                .map(|value| MultiValue::from_vec(vec![Value::Number(value as f64)]))
                .map_err(|_| fail(&limit));
        }
        if let Some(Value::Table(table)) = arguments.front() {
            let cost = table_cost(lua, table, &limit)?;
            limit.charge(cost)?;
        }
        call(lua, "time", arguments, &limit)
    })
}

fn date(lua: &Lua, budget: &Budget, runtime: &Arc<Runtime>) -> luaur::Result<Function> {
    let state = runtime.clone();
    let limit = budget.clone();
    lua.create_function(move |lua, arguments: MultiValue| {
        let format_cost = format_cost(arguments.front()).map_err(|_| fail(&limit))?;
        limit.charge(format_cost.max(1))?;
        let current = arguments
            .get(1)
            .is_none_or(|value| matches!(value, Value::Nil));
        let arguments = if current {
            let now = state.now().map_err(|_| fail(&limit))?;
            let mut values: Vec<Value> = arguments.into_iter().collect();
            if values.is_empty() {
                values.push(Value::Nil);
            }
            if values.len() == 1 {
                values.push(Value::Number(now as f64));
            } else {
                values[1] = Value::Number(now as f64);
            }
            MultiValue::from_vec(values)
        } else {
            validate_date_timestamp(lua, arguments.get(1), &limit)?;
            arguments
        };
        let values = call(lua, "date", arguments, &limit)?;
        if let Some(value) = values.front() {
            match value {
                Value::String(text) => limit.charge(text.as_bytes().len())?,
                Value::Table(_) => limit.charge(DATE_RESULT_FIELDS)?,
                _ => {}
            }
        }
        Ok(values)
    })
}

fn passthrough(
    lua: &Lua,
    name: &'static str,
    budget: &Budget,
    runtime: &Arc<Runtime>,
    live: bool,
) -> luaur::Result<Function> {
    let limit = budget.clone();
    let runtime = runtime.clone();
    lua.create_function(move |lua, arguments: MultiValue| {
        limit.charge(1)?;
        if live {
            runtime.live().map_err(|_| fail(&limit))?;
        }
        call(lua, name, arguments, &limit)
    })
}

fn call(
    lua: &Lua,
    name: &str,
    arguments: MultiValue,
    budget: &Budget,
) -> luaur::Result<MultiValue> {
    let original: Function = lua.named_registry_value(&format!("mock.native.os.{name}"))?;
    original
        .call(arguments)
        .map_err(|error| classify_native_error(error, budget))
}

fn classify_native_error(error: Error, budget: &Budget) -> Error {
    let error = budget.preserve_boundary_error(error);
    if matches!(&error, Error::RuntimeError(_)) {
        budget.recoverable_native_error();
        bad()
    } else {
        error
    }
}

fn table_cost(lua: &Lua, table: &Table, budget: &Budget) -> luaur::Result<usize> {
    if table.metatable().is_some() {
        return Err(fail(budget));
    }
    let mut count = 0usize;
    for entry in table.clone().pairs::<Value, Value>() {
        entry.map_err(|_| fail(budget))?;
        count += 1;
        if count > crate::value::MAX_ITEMS {
            return Err(fail(budget));
        }
    }
    for name in ["year", "month", "day", "hour", "min", "sec"] {
        let value: Value = table.raw_get(name)?;
        if matches!(value, Value::Nil) {
            continue;
        }
        let Some(number) = lua
            .coerce_number(value)
            .map_err(|error| classify_native_error(error, budget))?
        else {
            continue;
        };
        if !number.is_finite() || !(i32::MIN as f64..=i32::MAX as f64).contains(&number) {
            return Err(fail(budget));
        }
    }
    Ok(count.max(1))
}

fn format_cost(value: Option<&Value>) -> luaur::Result<usize> {
    let Some(Value::String(value)) = value else {
        return Ok(1);
    };
    let bytes = value.as_bytes();
    if bytes.len() > crate::value::MAX_STRING_BYTES {
        return Err(bad());
    }
    let mut estimate = 0usize;
    let mut index = 0usize;
    while index < bytes.len() && bytes[index] != 0 {
        if bytes[index] == b'%' && index + 1 < bytes.len() {
            estimate = estimate.saturating_add(if bytes[index + 1] == b'%' { 1 } else { 32 });
            index += 2;
        } else {
            estimate = estimate.saturating_add(1);
            index += 1;
        }
    }
    if estimate > crate::value::MAX_STRING_BYTES {
        Err(bad())
    } else {
        Ok(bytes.len().max(estimate).max(1))
    }
}

fn validate_date_timestamp(lua: &Lua, value: Option<&Value>, budget: &Budget) -> luaur::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if let Value::String(text) = value
        && text.as_bytes().len() > crate::value::MAX_STRING_BYTES
    {
        return Err(fail(budget));
    }
    let Some(value) = lua
        .coerce_number(value.clone())
        .map_err(|error| classify_native_error(error, budget))?
    else {
        return Ok(());
    };
    if value.is_finite() && value.abs() <= 9_007_199_254_740_991_f64 {
        Ok(())
    } else {
        Err(fail(budget))
    }
}

fn timestamp(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Integer(value) if (0..=9_007_199_254_740_991).contains(value) => Some(*value),
        Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && (0.0..=9_007_199_254_740_991_f64).contains(value) =>
        {
            let integer = *value as i64;
            (integer as f64 == *value).then_some(integer)
        }
        _ => None,
    }
}

fn options(value: Option<&Value>) -> Result<bool, ()> {
    let Some(value) = value else {
        return Ok(false);
    };
    if matches!(value, Value::Nil) {
        return Ok(false);
    }
    let Value::Table(table) = value else {
        return Err(());
    };
    if table.metatable().is_some() {
        return Err(());
    }
    let mut world = false;
    for entry in table.clone().pairs::<Value, Value>() {
        let (key, value) = entry.map_err(|_| ())?;
        let Value::String(key) = key else {
            return Err(());
        };
        if key.as_bytes() != b"world" {
            return Err(());
        }
        world = match value {
            Value::Boolean(value) => value,
            Value::Nil => false,
            _ => return Err(()),
        };
    }
    Ok(world)
}

fn bad() -> Error {
    Error::runtime("invalid bounded time call")
}

fn fail(budget: &Budget) -> Error {
    budget.runtime_failure();
    bad()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_allocator_errors_remain_sticky_error_kinds() {
        let lua = Lua::new_with(luaur::StdLib::ALL_SAFE, luaur::LuaOptions::default()).unwrap();
        let state = super::super::install_budget(
            &lua,
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        )
        .unwrap();
        let error = classify_native_error(
            Error::MemoryError("allocation failed".into()),
            &Budget::startup(state.clone()),
        );
        assert!(matches!(error, Error::MemoryError(_)));
        assert_eq!(
            state.borrow().failure(),
            Some(super::super::EvaluationFailure::Memory)
        );
    }
}
