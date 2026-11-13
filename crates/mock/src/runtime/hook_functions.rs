//! Validate log/reject arguments and phase authority in Rust; preserve the first rejection.

use std::cell::RefCell;
use std::rc::Rc;

use chigiri::core::{Object, Value};
use luaur::{Error, Lua, MultiValue, Table};

use super::hook::HookLog;
use super::state::Host;
use crate::api::hooks;
use crate::codec::Codec;
use crate::hooks::Rejection;
use crate::protected_call::State;

pub(super) fn attach(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    context: &Table,
) -> luaur::Result<()> {
    let slot = host.borrow().hook.as_ref().ok_or_else(bad)?.slot;
    if slot.allows_reject() {
        let state = host.clone();
        let codec = Rc::downgrade(codec);
        let reject =
            lua.create_function(move |lua, arguments: MultiValue| -> luaur::Result<()> {
                let mut state = state.borrow_mut();
                state.call(lua)?;
                if state
                    .hook
                    .as_ref()
                    .is_some_and(|hook| hook.rejection.is_some())
                {
                    state.hook.as_mut().ok_or_else(bad)?.reject_unwinding = true;
                    return Err(bad());
                }
                // Preserve a sanitized 500 rejection if argument decoding fails or the codec is unavailable.
                state.hook.as_mut().ok_or_else(bad)?.rejection = Some(Rejection::invalid());
                let decoded = (|| {
                    if !crate::api::hook_values::REJECT_FUNCTION.accepts_arity(arguments.len()) {
                        return Err(bad());
                    }
                    let codec = codec.upgrade().ok_or_else(bad)?;
                    let mut codec = codec.borrow_mut();
                    let raw = codec
                        .scope()
                        .callback(lua, arguments.front().ok_or_else(bad)?.clone())
                        .map_err(|_| bad())?;
                    match codec.decode_object(&state.ir, &raw).map_err(|_| bad())? {
                        Value::Object(object) => Ok(Rejection::decode(&object)),
                        _ => Err(bad()),
                    }
                })();
                if let Ok(rejection) = decoded {
                    state.hook.as_mut().ok_or_else(bad)?.rejection = Some(rejection);
                }
                if state
                    .hook
                    .as_ref()
                    .and_then(|hook| hook.rejection.as_ref())
                    .is_some_and(|rejection| rejection.invalid)
                {
                    state.capture_stack(lua, 1, "host");
                }
                state.hook.as_mut().ok_or_else(bad)?.reject_unwinding = true;
                Err(bad())
            })?;
        context.raw_set(hooks::REJECT, reject)?;
    }
    let sink = host.borrow().hook.as_ref().ok_or_else(bad)?.log.clone();
    attach_log(lua, host, codec, context, sink)
}

pub(super) fn attach_log(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    context: &Table,
    sink: super::HookLogSink,
) -> luaur::Result<()> {
    let state = host.clone();
    let codec = Rc::downgrade(codec);
    let log = lua.create_function(move |lua, arguments: MultiValue| {
        let mut state = state.borrow_mut();
        let result = (|| {
            state.call(lua)?;
            if !crate::api::hook_values::LOG_FUNCTION.accepts_arity(arguments.len()) {
                return Err(bad());
            }
            let codec = codec.upgrade().ok_or_else(bad)?;
            let mut codec = codec.borrow_mut();
            let scope = codec.scope();
            let mut values = arguments.into_iter();
            let mut text = || {
                let value = scope
                    .callback(lua, values.next().ok_or_else(bad)?)
                    .map_err(|_| bad())?;
                match codec.decode_json(&state.ir, &value).map_err(|_| bad())? {
                    Value::String(value) => Ok(value),
                    _ => Err(bad()),
                }
            };
            let level = text()?;
            let message = text()?;
            let level = hooks::LEVELS
                .into_iter()
                .find(|name| *name == level)
                .ok_or_else(bad)?;
            let fields = match values.next() {
                None | Some(luaur::Value::Nil) => Object::new(),
                Some(value) => {
                    let value = scope.callback(lua, value).map_err(|_| bad())?;
                    let Value::Object(fields) =
                        codec.decode_object(&state.ir, &value).map_err(|_| bad())?
                    else {
                        return Err(bad());
                    };
                    fields
                }
            };
            let sink = sink.clone();
            Ok((
                sink,
                HookLog {
                    level,
                    message,
                    fields,
                },
            ))
        })();
        if result.is_err() {
            state.fail();
            state.capture_stack(lua, 1, "host");
        }
        drop(state);
        let (sink, record) = result?;
        sink(record);
        Ok(())
    })?;
    context.raw_set(hooks::LOG, log)
}

fn bad() -> Error {
    Error::runtime("invalid hook host call")
}
