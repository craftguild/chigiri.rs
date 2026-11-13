//! Restrict raw value-handle access to verified VM scopes.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use luaur::{Function, Table, Value as LuaValue};

use super::{Codec, vm_error};
use crate::value::ValueError;

/// A value handle owned by one codec and VM.
#[derive(Clone)]
pub struct VmValue {
    value: LuaValue,
    scope: Rc<Identity>,
}

impl VmValue {
    /// Return the raw handle for use within the owning VM.
    pub fn as_lua(&self) -> &LuaValue {
        &self.value
    }
}

/// Scope used to re-enter host callbacks and verify VM ownership without exposing the codec.
#[derive(Clone)]
pub struct VmScope {
    identity: Rc<Identity>,
}

struct Identity {
    thread: usize,
    failed: Cell<bool>,
}

impl VmScope {
    pub(super) fn new(lua: &luaur::Lua) -> Self {
        Self {
            identity: Rc::new(Identity {
                thread: lua.current_thread().to_pointer() as usize,
                failed: Cell::new(false),
            }),
        }
    }

    /// Call a function only when it and every argument belong to this VM.
    #[cfg(test)]
    pub fn call(&self, function: &Function, arguments: &[VmValue]) -> Result<VmValue, ValueError> {
        self.call_observed(function, arguments, |_| {})
    }

    pub(crate) fn call_observed(
        &self,
        function: &Function,
        arguments: &[VmValue],
        error: impl FnOnce(&luaur::Error),
    ) -> Result<VmValue, ValueError> {
        self.call_result_observed(function, arguments, false, error)
    }

    pub(crate) fn call_void_observed(
        &self,
        function: &Function,
        arguments: &[VmValue],
        error: impl FnOnce(&luaur::Error),
    ) -> Result<VmValue, ValueError> {
        self.call_result_observed(function, arguments, true, error)
    }

    fn call_result_observed(
        &self,
        function: &Function,
        arguments: &[VmValue],
        void: bool,
        error: impl FnOnce(&luaur::Error),
    ) -> Result<VmValue, ValueError> {
        self.run(|| {
            if !self.same_lua(&function.lua()) || arguments.len() > 3 {
                return Err(ValueError("foreign VM function or argument count"));
            }
            let mut raw = luaur::MultiValue::new();
            for value in arguments {
                raw.push_back(self.value(value)?.clone());
            }
            let result = if void {
                function.call::<luaur::MultiValue>(raw).and_then(|values| {
                    if values.is_empty() {
                        Ok(LuaValue::Nil)
                    } else {
                        Err(luaur::Error::runtime("consumer must return no value"))
                    }
                })
            } else {
                function.call::<LuaValue>(raw)
            };
            result.map(|value| self.wrap(value)).map_err(|raw| {
                error(&raw);
                vm_error(raw)
            })
        })
    }

    /// Wrap only a value passed by Lua to this crate's host callback.
    pub(crate) fn callback(
        &self,
        lua: &luaur::Lua,
        value: LuaValue,
    ) -> Result<VmValue, ValueError> {
        self.run(|| {
            if !self.same_lua(lua) {
                return Err(ValueError("foreign VM callback"));
            }
            Ok(self.wrap(value))
        })
    }

    /// Wrap a table after verifying that it belongs to this VM.
    pub fn table(&self, table: &Table) -> Result<VmValue, ValueError> {
        self.run(|| {
            if !self.same_lua(&table.lua()) {
                return Err(ValueError("foreign VM table"));
            }
            Ok(self.wrap(LuaValue::Table(table.clone())))
        })
    }

    pub(super) fn same_lua(&self, lua: &luaur::Lua) -> bool {
        lua.current_thread().to_pointer() as usize == self.identity.thread
    }

    pub(super) fn value<'a>(&self, value: &'a VmValue) -> Result<&'a LuaValue, ValueError> {
        if !Rc::ptr_eq(&self.identity, &value.scope) {
            return Err(ValueError("foreign VM value"));
        }
        Ok(&value.value)
    }

    pub(super) fn check(&self) -> Result<(), ValueError> {
        if self.identity.failed.get() {
            Err(ValueError("VM value scope failed"))
        } else {
            Ok(())
        }
    }

    pub(super) fn fail(&self) {
        self.identity.failed.set(true);
    }

    fn run<T>(&self, operation: impl FnOnce() -> Result<T, ValueError>) -> Result<T, ValueError> {
        self.check()?;
        let result = catch_unwind(AssertUnwindSafe(operation))
            .unwrap_or(Err(ValueError("VM allocation failed")));
        match result {
            Ok(value) => {
                // Preserve a host failure captured by the script in the shared scope.
                self.check()?;
                Ok(value)
            }
            Err(error) => {
                self.fail();
                Err(error)
            }
        }
    }

    pub(super) fn wrap(&self, value: LuaValue) -> VmValue {
        VmValue {
            value,
            scope: self.identity.clone(),
        }
    }
}

impl Codec {
    /// Return a cloneable VM-ownership scope so callbacks need not retain a codec borrow.
    pub fn scope(&self) -> VmScope {
        self.scope.clone()
    }

    #[cfg(test)]
    pub(super) fn wrap_for_test(&self, value: LuaValue) -> VmValue {
        self.scope.wrap(value)
    }
}
