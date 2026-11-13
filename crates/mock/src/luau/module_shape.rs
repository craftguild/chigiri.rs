//! Validate the shared closed resolver/hook module shape without invoking its entry point.

use luaur::{Function, Value};

use super::ScriptError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Entrypoint {
    Resolve,
    Subscribe,
    Consume,
}

#[derive(Clone, Copy)]
pub(super) enum ModuleContract {
    Ordinary,
    Subscription { root_as_nested: bool },
    Consumer,
}

pub(super) struct ResolverModule {
    resolve: Option<Function>,
    subscribe: Option<Function>,
    consume: Option<Function>,
}

impl ResolverModule {
    pub fn take(self, entrypoint: Entrypoint) -> Result<Function, ScriptError> {
        match entrypoint {
            Entrypoint::Resolve => self.resolve,
            Entrypoint::Subscribe => self.subscribe,
            Entrypoint::Consume => self.consume,
        }
        .ok_or_else(invalid)
    }
}

pub(super) fn decode(
    value: Value,
    length: &Function,
    entrypoint: &'static str,
    mut cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<Function, ScriptError> {
    let invalid = || ScriptError::new("module", "closed function module required");
    let mut observed = |raw: luaur::Error| {
        let mut failure = invalid();
        failure.cancelled = cancelled(&raw);
        failure
    };
    let Value::Table(table) = value else {
        return Err(invalid());
    };
    if table.metatable().is_some() {
        return Err(invalid());
    }
    let mut function = None;
    for (index, entry) in table.pairs::<Value, Value>().enumerate() {
        if index == 1 {
            return Err(invalid());
        }
        let (key, value) = entry.map_err(&mut observed)?;
        let Value::String(key) = key else {
            return Err(invalid());
        };
        if length.call::<usize>(key.clone()).map_err(&mut observed)? > 7 {
            return Err(invalid());
        }
        let key = key.to_str().map_err(&mut observed)?;
        match (key.as_str(), value) {
            (name, Value::Function(value)) if name == entrypoint => function = Some(value),
            _ => return Err(invalid()),
        }
    }
    function.ok_or_else(invalid)
}

pub(super) fn decode_resolver(
    value: Value,
    length: &Function,
    contract: ModuleContract,
    mut cancelled: impl FnMut(&luaur::Error) -> bool,
) -> Result<ResolverModule, ScriptError> {
    let mut observed = |raw: luaur::Error| {
        let mut failure = invalid();
        failure.cancelled = cancelled(&raw);
        failure
    };
    let Value::Table(table) = value else {
        return Err(invalid());
    };
    if table.metatable().is_some() {
        return Err(invalid());
    }
    let mut module = ResolverModule {
        resolve: None,
        subscribe: None,
        consume: None,
    };
    for (index, entry) in table.pairs::<Value, Value>().enumerate() {
        if index == 2 {
            return Err(invalid());
        }
        let (key, value) = entry.map_err(&mut observed)?;
        let (Value::String(key), Value::Function(value)) = (key, value) else {
            return Err(invalid());
        };
        if length.call::<usize>(key.clone()).map_err(&mut observed)? > "subscribe".len() {
            return Err(invalid());
        }
        match key.to_str().map_err(&mut observed)?.as_str() {
            "resolve" if module.resolve.is_none() => module.resolve = Some(value),
            "subscribe" if module.subscribe.is_none() => module.subscribe = Some(value),
            "consume" if module.consume.is_none() => module.consume = Some(value),
            _ => return Err(invalid()),
        }
    }
    let matches = match contract {
        ModuleContract::Ordinary => {
            module.resolve.is_some() && module.subscribe.is_none() && module.consume.is_none()
        }
        ModuleContract::Subscription {
            root_as_nested: false,
        } => module.resolve.is_none() && module.subscribe.is_some() && module.consume.is_none(),
        ModuleContract::Subscription {
            root_as_nested: true,
        } => module.resolve.is_some() && module.subscribe.is_some() && module.consume.is_none(),
        ModuleContract::Consumer => {
            module.resolve.is_none() && module.subscribe.is_some() && module.consume.is_some()
        }
    };
    matches.then_some(module).ok_or_else(invalid)
}

fn invalid() -> ScriptError {
    ScriptError::new("module", "closed function module required")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_shape_forwards_length_function_errors_to_the_typed_observer() {
        let _case = crate::vm_gate::test_case();
        let _lease = crate::vm_gate::test_guard();
        let lua = luaur::Lua::new();
        lua.set_memory_limit(crate::value::MAX_BYTES).unwrap();
        let module = lua
            .load("return {resolve=function() return nil end}")
            .eval()
            .unwrap();
        let length = lua
            .load("return function(_) local s='x' for _=1,30 do s=s..s end return 0 end")
            .eval()
            .unwrap();
        let mut observed = false;
        let result = decode(module, &length, "resolve", |raw| {
            assert!(matches!(raw, luaur::Error::MemoryError(_)));
            observed = true;
            false
        });
        assert!(result.is_err());
        assert!(observed);
    }
}
