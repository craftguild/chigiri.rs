//! Capture one field's complete sequence definition during its short setup VM.

use std::cell::RefCell;
use std::rc::Rc;

use luaur::{Lua, MultiValue, Table, Value as LuaValue};

use super::BindingCollector;
use super::context::{decode_overflow, invalid};
use crate::api::subscriptions as contract;
use crate::codec::{Codec, HostStringKind};
use crate::protected_call::State;
use crate::publication::DefinitionBuilder;
use crate::runtime::state::{Host, Session};

struct Input {
    values: Table,
    interval_ms: u64,
    looping: bool,
    overflow: chigiri::SubscriptionOverflow,
}

pub(super) fn capability(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    collector: &BindingCollector,
) -> luaur::Result<Table> {
    let generator = lua.create_table_result()?;
    let sequence = lua.create_table_result()?;
    let field = host.borrow().ir.scripts()[collector.script().0].field;
    let name = crate::events::payload::field_type_name(&host.borrow().ir, field).to_owned();
    let state = host.clone();
    let codec = Rc::downgrade(codec);
    let bindings = collector.clone();
    let generate = lua.create_function(move |lua, arguments: MultiValue| {
        let mut state = state.borrow_mut();
        let result = (|| {
            state.call(lua)?;
            if state.writable() || arguments.len() != 1 {
                return Err(invalid());
            }
            let input = match arguments.front() {
                Some(LuaValue::Table(input)) if input.metatable().is_none() => input,
                _ => return Err(invalid()),
            };
            let codec = codec.upgrade().ok_or_else(invalid)?;
            let mut codec = codec.borrow_mut();
            let input = decode(&mut codec, input)?;
            let count = codec
                .source_array_len(&input.values, contract::MAX_SEQUENCE_VALUES)
                .map_err(|_| invalid())?;
            let ir = state.ir.clone();
            let (limits, retention) = bindings.policy();
            let mut definition = DefinitionBuilder::new(
                &ir,
                field,
                count,
                input.interval_ms,
                input.looping,
                limits,
                retention,
            )
            .map_err(|_| invalid())?;
            for index in 1..=count {
                let value = codec
                    .scope()
                    .callback(lua, input.values.raw_get(index)?)
                    .map_err(|_| invalid())?;
                let (value, provenance) = codec
                    .decode_output(&ir, &ir.graph().fields()[field.0].ty, &value)
                    .map_err(|_| invalid())?;
                let Session::Read(reader) = &mut state.session else {
                    return Err(invalid());
                };
                let output = reader
                    .output_provenance(field, &value, &provenance)
                    .map_err(|_| invalid())?;
                definition.push(&ir, &output).map_err(|_| invalid())?;
            }
            bindings.push_sequence(
                lua,
                definition.seal().map_err(|_| invalid())?,
                input.overflow,
            )
        })();
        if result.is_err() {
            if let Some(codec) = codec.upgrade() {
                codec.borrow_mut().fail();
            }
            state.fail();
            state.capture_stack(lua, 1, "host");
        }
        result
    })?;
    sequence.raw_set(name, generate)?;
    sequence.set_readonly(true);
    generator.raw_set(contract::SEQUENCE, sequence)?;
    generator.set_readonly(true);
    Ok(generator)
}

fn decode(codec: &mut Codec, input: &Table) -> luaur::Result<Input> {
    let mut values = None;
    let mut interval_ms = None;
    let mut looping = None;
    let mut overflow = None;
    for (index, entry) in input.pairs::<LuaValue, LuaValue>().enumerate() {
        if index == 4 {
            return Err(invalid());
        }
        let (key, value) = entry?;
        let LuaValue::String(key) = key else {
            return Err(invalid());
        };
        let key = codec
            .bounded_host_string(&key, contract::INTERVAL_MS.len(), HostStringKind::Key)
            .map_err(|_| invalid())?;
        match (key.as_str(), value) {
            (contract::VALUES, LuaValue::Table(table)) if values.is_none() => values = Some(table),
            (contract::INTERVAL_MS, value) if interval_ms.is_none() => {
                interval_ms = Some(interval(value)?)
            }
            (contract::LOOP, LuaValue::Boolean(value)) if looping.is_none() => {
                looping = Some(value)
            }
            (contract::OVERFLOW, value) if overflow.is_none() => {
                overflow = Some(decode_overflow(codec, value)?)
            }
            _ => return Err(invalid()),
        }
    }
    Ok(Input {
        values: values.ok_or_else(invalid)?,
        interval_ms: interval_ms.ok_or_else(invalid)?,
        looping: looping.unwrap_or(false),
        overflow: overflow.unwrap_or(chigiri::SubscriptionOverflow::Backpressure),
    })
}

fn interval(value: LuaValue) -> luaur::Result<u64> {
    match value {
        LuaValue::Integer(value) if (1..=contract::MAX_INTERVAL_MS as i64).contains(&value) => {
            Ok(value as u64)
        }
        LuaValue::Number(value)
            if value.fract() == 0.0
                && (1.0..=contract::MAX_INTERVAL_MS as f64).contains(&value) =>
        {
            Ok(value as u64)
        }
        _ => Err(invalid()),
    }
}
