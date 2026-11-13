//! Create model CRUD closures and revalidate receiver, arity, and execution privileges in Rust.

use std::cell::RefCell;
use std::rc::Rc;

use super::methods::invoke;
use super::state::Host;
use crate::api::context as contract;
use crate::api::{METHODS, MethodSpec};
use crate::codec::{Codec, JsonShape};
use crate::ir::ModelId;
use crate::protected_call::State;
use crate::value::ValueError;
use chigiri::core::Value;
use luaur::{Error, Lua, MultiValue, Table, Value as LuaValue};

pub(super) fn context(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
) -> luaur::Result<Table> {
    context_with(lua, host, codec, None, None)
}

pub(super) fn resolver_context(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    publication: Option<Table>,
) -> luaur::Result<Table> {
    context_with(lua, host, codec, None, publication)
}

pub(super) fn subscription_setup_context(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    capability: (Table, Table),
) -> luaur::Result<Table> {
    context_with(lua, host, codec, Some(capability), None)
}

fn context_with(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    subscription: Option<(Table, Table)>,
    publication: Option<Table>,
) -> luaur::Result<Table> {
    let context = lua.create_table_result()?;
    context.raw_set(
        contract::SCOPE,
        if subscription.is_some() {
            crate::api::subscriptions::SETUP_SCOPE
        } else if host.borrow().writable() {
            contract::MUTATION
        } else {
            contract::READ
        },
    )?;
    context.raw_set(contract::NULL, codec.borrow().null().as_lua().clone())?;
    let state = codec
        .borrow_mut()
        .readonly_object(&host.borrow().request_state)
        .map_err(value_error)?;
    context.raw_set(crate::api::hooks::STATE, state.as_lua().clone())?;
    let request = match &host.borrow().request {
        Some(request) => codec.borrow_mut().readonly(request).map_err(value_error)?,
        None => codec.borrow().null(),
    };
    context.raw_set(contract::REQUEST, request.as_lua().clone())?;
    attach_fail(lua, host, codec, &context)?;
    for constructor in contract::CONSTRUCTORS {
        let shape = match constructor.shape {
            contract::JsonKind::Object => JsonShape::Object,
            contract::JsonKind::Array => JsonShape::Array,
        };
        let state = host.clone();
        let codec = Rc::downgrade(codec);
        let function = lua.create_function(move |lua, arguments: MultiValue| {
            let mut state = state.borrow_mut();
            let result = (|| {
                state.call(lua)?;
                if arguments.len() != constructor.arity {
                    return Err(bad());
                }
                let Some(LuaValue::Table(table)) = arguments.front() else {
                    return Err(bad());
                };
                codec
                    .upgrade()
                    .ok_or_else(bad)?
                    .borrow_mut()
                    .mark(table, shape)
                    .map_err(value_error)?;
                Ok(LuaValue::Table(table.clone()))
            })();
            if result.is_err() {
                state.fail();
                state.capture_stack(lua, 1, "host");
            }
            result
        })?;
        context.raw_set(constructor.name, function)?;
    }
    let models = lua.create_table_result()?;
    let ir = host.borrow().ir.clone();
    for (index, model) in ir.models().iter().enumerate() {
        let table = lua.create_table_result()?;
        for spec in METHODS {
            if spec.effect.allowed(host.borrow().writable())
                && spec.eligibility.allows(&ir, ModelId(index))
            {
                attach(lua, host, codec, &table, ModelId(index), spec)?;
            }
        }
        table.set_readonly(true);
        models.raw_set(ir.graph().types()[model.ty.0].name.as_ref(), table)?;
    }
    models.set_readonly(true);
    context.raw_set(contract::MODELS, models)?;
    if let Some((pubsub, generator)) = subscription {
        context.raw_set(crate::api::subscriptions::PUBSUB, pubsub)?;
        context.raw_set(crate::api::subscriptions::GENERATOR, generator)?;
    }
    if let Some(publication) = publication {
        context.raw_set(crate::api::subscriptions::PUBSUB, publication)?;
    }
    context.set_readonly(true);
    Ok(context)
}

fn attach_fail(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    context: &Table,
) -> luaur::Result<()> {
    let state = host.clone();
    let codec = Rc::downgrade(codec);
    let function = lua.create_function(move |lua, arguments: MultiValue| -> luaur::Result<()> {
        let mut state = state.borrow_mut();
        state.call(lua)?;
        if state.business.get().is_some() {
            state.business_unwinding = true;
            return Err(business_control());
        }
        let decoded = (|| {
            if arguments.len() != 1 {
                return Err(bad());
            }
            let codec = codec.upgrade().ok_or_else(bad)?;
            let mut codec = codec.borrow_mut();
            let raw = codec
                .scope()
                .callback(lua, arguments.front().ok_or_else(bad)?.clone())
                .map_err(|_| bad())?;
            let Value::Object(object) = codec.decode_object(&state.ir, &raw).map_err(|_| bad())?
            else {
                return Err(bad());
            };
            super::business::BusinessFailure::decode(&object, state.business_path.clone())
                .map_err(|_| bad())
        })();
        match decoded {
            Ok(failure) => {
                state.business.record(failure);
                state.business_unwinding = true;
                Err(business_control())
            }
            Err(error) => {
                state.fail();
                state.capture_stack(lua, 1, "host");
                Err(error)
            }
        }
    })?;
    context.raw_set(contract::FAIL, function)
}

fn attach(
    lua: &Lua,
    host: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    table: &Table,
    model: ModelId,
    spec: &'static MethodSpec,
) -> luaur::Result<()> {
    let state = host.clone();
    // Capture the codec weakly to avoid a strong VM→closure→Codec→VM cycle.
    let codec = Rc::downgrade(codec);
    let identity = table.to_pointer() as usize;
    let function = lua.create_function(move |lua, arguments: MultiValue| {
        let mut state = state.borrow_mut();
        let result = (|| {
            state.call(lua)?;
            if !spec.effect.allowed(state.writable()) || arguments.len() != spec.arity() || !matches!(arguments.front(), Some(LuaValue::Table(receiver)) if receiver.to_pointer() as usize == identity) {
                return Err(bad());
            }
            let codec = codec.upgrade().ok_or_else(bad)?;
            let mut codec = codec.borrow_mut();
            let scope = codec.scope();
            // Arguments contain only values supplied by this Lua callback and retain no unrelated raw handles.
            let args = arguments.into_iter().skip(1).map(|value| scope.callback(lua, value)).collect::<Result<Vec<_>, _>>().map_err(value_error)?;
            let value = invoke(&mut state, &mut codec, model, spec, &args).map_err(value_error)?;
            Ok(value.as_lua().clone())
        })();
        if result.is_err() {
            state.fail();
            state.capture_stack(lua, 1, "host");
        }
        result
    })?;
    table.raw_set(spec.name, function)
}
fn bad() -> Error {
    Error::runtime("invalid host call")
}
fn value_error(_: ValueError) -> Error {
    Error::runtime("host value or store operation failed")
}

fn business_control() -> Error {
    Error::runtime("business rejection")
}
