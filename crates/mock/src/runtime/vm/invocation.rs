//! Select phase-specific context, arguments and results within the same fresh VM.

use std::cell::RefCell;
use std::rc::Rc;

use chigiri::core::{Object, Value};
use luaur::{Lua, Table, Value as LuaValue};

use super::{CallKind, CallOutput, VmOutput};
use crate::codec::{Codec, VmValue};
use crate::luau::{Script, ScriptError};
use crate::runtime::{
    context as host_context, hook_functions, publication, state::Host, subscription_setup,
};

pub(super) fn context(
    lua: &Lua,
    state: &Rc<RefCell<Host>>,
    codec: &Rc<RefCell<Codec>>,
    kind: CallKind<'_>,
) -> luaur::Result<Table> {
    let context = match kind {
        CallKind::Resolve(_) | CallKind::Consume(_) => {
            let publication = publication::capability(lua, state, codec)?;
            host_context::resolver_context(lua, state, codec, publication)?
        }
        CallKind::Subscribe { bindings, .. } => {
            let capability = subscription_setup::capability(lua, state, codec, bindings)?;
            host_context::subscription_setup_context(lua, state, codec, capability)?
        }
    };
    let (log, scope) = match kind {
        CallKind::Subscribe { log: Some(log), .. } => (log, crate::api::consumers::SETUP_SCOPE),
        CallKind::Consume(call) => (call.log, crate::api::consumers::EXECUTION_SCOPE),
        _ => return Ok(context),
    };
    // This context has not reached authored code yet. Extend it before sealing.
    context.set_readonly(false);
    context.raw_set(crate::api::context::SCOPE, scope)?;
    hook_functions::attach_log(lua, state, codec, &context, log.clone())?;
    if let CallKind::Consume(call) = kind {
        let metadata = codec
            .borrow_mut()
            .readonly_object(call.metadata)
            .map_err(|_| luaur::Error::runtime("consumer metadata conversion failed"))?;
        context.raw_set(crate::api::consumers::EVENT, metadata.as_lua().clone())?;
    }
    context.set_readonly(true);
    Ok(context)
}

pub(super) fn arguments(
    codec: &mut Codec,
    context: &Table,
    args: &Object,
    kind: CallKind<'_>,
) -> Result<Vec<VmValue>, crate::value::ValueError> {
    let mut arguments = vec![codec.scope().table(context)?];
    match kind {
        CallKind::Resolve(parent) => {
            arguments.push(codec.encode(&Value::Object(args.clone()))?);
            if let Some(parent) = parent {
                arguments.push(codec.encode_parent(parent)?);
            }
        }
        CallKind::Subscribe { log: None, .. } => {
            arguments.push(codec.encode(&Value::Object(args.clone()))?);
        }
        CallKind::Subscribe { log: Some(_), .. } => {}
        CallKind::Consume(call) => arguments.push(codec.encode_output(call.event)?),
    }
    Ok(arguments)
}

pub(super) fn output(
    script: &Script,
    codec: &mut Codec,
    result: &VmValue,
    kind: CallKind<'_>,
) -> Result<CallOutput, ScriptError> {
    match kind {
        CallKind::Resolve(_) => {
            let field = &script.ir().graph().fields()[script.ir().scripts()[script.id().0].field.0];
            let (value, provenance) = codec
                .decode_output(script.ir(), &field.ty, result)
                .map_err(|_| script.error("value", "return conversion failed", 1, 1))?;
            Ok(CallOutput::Resolve(VmOutput { value, provenance }))
        }
        CallKind::Subscribe { bindings, .. } => bindings
            .take(result.as_lua())
            .map(CallOutput::Subscribe)
            .map_err(|_| script.error("value", "binding result required", 1, 1)),
        CallKind::Consume(_) if matches!(result.as_lua(), LuaValue::Nil) => Ok(CallOutput::Consume),
        CallKind::Consume(_) => Err(script.error("value", "consumer must return no value", 1, 1)),
    }
}
