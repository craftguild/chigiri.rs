//! Install the setup-only opaque binding capability into a read-only context.

use std::cell::RefCell;
use std::rc::Rc;

use luaur::{Error, Lua, MultiValue, Table, Value as LuaValue};

use super::binding::BindingCollector;
use crate::api::subscriptions as contract;
use crate::codec::{Codec, HostStringKind};
use crate::protected_call::State;
use crate::runtime::state::Host;

const KEY_MAX_BYTES: usize = crate::api::consumers::ALL_TOPICS.len();
const TOPIC_MAX_BYTES: usize = 256;
const OVERFLOW_MAX_BYTES: usize = contract::BACKPRESSURE.len();

pub(in crate::runtime) fn capability(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    collector: &BindingCollector,
) -> luaur::Result<(Table, Table)> {
    let generator = super::generator::capability(lua, host, codec, collector)?;
    let capability = lua.create_table_result()?;
    let select = lua.create_table_result()?;
    let field = host.borrow().ir.scripts()[collector.script().0].field;
    let name = crate::events::payload::field_type_name(&host.borrow().ir, field).to_owned();
    let state = host.clone();
    let codec = Rc::downgrade(codec);
    let bindings = collector.clone();
    let bind = lua.create_function(move |lua, arguments: MultiValue| {
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
            let (topic, overflow) =
                decode(&mut codec.borrow_mut(), input, bindings.allows_all_topics())?;
            match topic {
                Some(topic) => bindings.push(lua, topic, overflow),
                None => bindings.push_all_topics(lua, overflow),
            }
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
    select.raw_set(name, bind)?;
    select.set_readonly(true);
    capability.raw_set(contract::SUBSCRIBE, select)?;
    capability.set_readonly(true);
    Ok((capability, generator))
}

fn decode(
    codec: &mut Codec,
    input: &Table,
    consumer: bool,
) -> luaur::Result<(Option<Box<str>>, chigiri::SubscriptionOverflow)> {
    let mut topic = None;
    let mut all_topics = false;
    let mut overflow = None;
    for (index, entry) in input.pairs::<LuaValue, LuaValue>().enumerate() {
        if index == 2 {
            return Err(invalid());
        }
        let (key, value) = entry.map_err(|_| invalid())?;
        let LuaValue::String(key) = key else {
            return Err(invalid());
        };
        let key = codec
            .bounded_host_string(&key, KEY_MAX_BYTES, HostStringKind::Key)
            .map_err(|_| invalid())?;
        match key.as_str() {
            contract::TOPIC if topic.is_none() => {
                let LuaValue::String(value) = value else {
                    return Err(invalid());
                };
                let value = codec
                    .bounded_host_string(&value, TOPIC_MAX_BYTES, HostStringKind::Value)
                    .map_err(|_| invalid())?;
                if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_control()) {
                    return Err(invalid());
                }
                topic = Some(value.into_boxed_str());
            }
            crate::api::consumers::ALL_TOPICS if consumer && !all_topics => {
                if !matches!(value, LuaValue::Boolean(true)) {
                    return Err(invalid());
                }
                all_topics = true;
            }
            contract::OVERFLOW if overflow.is_none() => {
                overflow = Some(decode_overflow(codec, value)?);
            }
            _ => return Err(invalid()),
        }
    }
    if topic.is_some() == all_topics {
        return Err(invalid());
    }
    Ok((
        topic,
        overflow.unwrap_or(chigiri::SubscriptionOverflow::Backpressure),
    ))
}

pub(super) fn decode_overflow(
    codec: &mut Codec,
    value: LuaValue,
) -> luaur::Result<chigiri::SubscriptionOverflow> {
    let LuaValue::String(value) = value else {
        return Err(invalid());
    };
    let value = codec
        .bounded_host_string(&value, OVERFLOW_MAX_BYTES, HostStringKind::Value)
        .map_err(|_| invalid())?;
    match value.as_str() {
        contract::BACKPRESSURE => Ok(chigiri::SubscriptionOverflow::Backpressure),
        contract::DROP_OLDEST => Ok(chigiri::SubscriptionOverflow::DropOldest),
        _ => Err(invalid()),
    }
}

pub(super) fn invalid() -> Error {
    Error::runtime("invalid subscription source")
}
