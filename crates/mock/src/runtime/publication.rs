//! Install SDL-type publication closures only in explicitly authorized write VMs.

use std::cell::RefCell;
use std::rc::Rc;

use luaur::{Error, Lua, MultiValue, Table, Value as LuaValue};

use super::state::Host;
use crate::api::subscriptions as contract;
use crate::codec::{Codec, HostStringKind};
use crate::protected_call::State;

const KEY_MAX_BYTES: usize = contract::PAYLOAD.len();
const TOPIC_MAX_BYTES: usize = 256;

pub(super) fn capability(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
) -> luaur::Result<Option<Table>> {
    let (ir, publications) = {
        let host = host.borrow();
        if !host.writable() {
            return Ok(None);
        }
        let Some(publications) = host.publications.clone() else {
            return Ok(None);
        };
        (host.ir.clone(), publications)
    };
    let group = lua.create_table_result()?;
    let mut fields = 0usize;
    for event in crate::events::payload::event_types(&ir) {
        let name = ir.graph().types()[event.named.0].name.clone();
        let state = host.clone();
        let codec = Rc::downgrade(codec);
        let ir = ir.clone();
        let publications = publications.clone();
        let function = lua.create_function(move |lua, arguments: MultiValue| {
            let mut state = state.borrow_mut();
            let result = (|| {
                state.call(lua)?;
                if arguments.len() != 1 {
                    return Err(invalid());
                }
                let input = match arguments.front() {
                    Some(LuaValue::Table(input)) if input.metatable().is_none() => input,
                    _ => return Err(invalid()),
                };
                let codec = codec.upgrade().ok_or_else(invalid)?;
                let mut codec = codec.borrow_mut();
                let (topic, payload) = decode(&mut codec, input)?;
                let raw = codec
                    .scope()
                    .callback(lua, payload)
                    .map_err(|_| invalid())?;
                let candidates = codec
                    .decode_event(&ir, &event.fields, &raw)
                    .map_err(|_| invalid())?;
                let (matching, output) = state
                    .write()
                    .and_then(|transaction| {
                        transaction
                            .publication_event(&candidates)
                            .map_err(|_| crate::value::ValueError("invalid publication payload"))
                    })
                    .map_err(|_| invalid())?;
                publications
                    .push_matching(&ir, &matching, &topic, &output)
                    .map_err(|_| invalid())?;
                Ok(())
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
        group.raw_set(name.as_ref(), function)?;
        fields += 1;
    }
    if fields == 0 {
        return Ok(None);
    }
    group.set_readonly(true);
    let capability = lua.create_table_result()?;
    capability.raw_set(contract::PUBLISH, group)?;
    capability.set_readonly(true);
    Ok(Some(capability))
}

fn decode(codec: &mut Codec, input: &Table) -> luaur::Result<(String, LuaValue)> {
    let mut topic = None;
    let mut payload = None;
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
                topic = Some(
                    codec
                        .bounded_host_string(&value, TOPIC_MAX_BYTES, HostStringKind::Value)
                        .map_err(|_| invalid())?,
                );
            }
            contract::PAYLOAD if payload.is_none() => payload = Some(value),
            _ => return Err(invalid()),
        }
    }
    Ok((topic.ok_or_else(invalid)?, payload.ok_or_else(invalid)?))
}

fn invalid() -> Error {
    Error::runtime("invalid publication")
}
