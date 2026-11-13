//! Enforce Rust/Luau re-entry limits and retain budget failures across `pcall` boundaries.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use luaur::{Error, Lua, MultiValue, Value};

pub(crate) trait State {
    fn observe_line(&mut self, lua: &Lua, level: usize);
    fn fail(&mut self);
    /// Record an explicit script error before the VM converts it to a protected-call result.
    fn explicit_error(&mut self) {}
    /// Record a re-entry limit using the execution context's specific failure class.
    fn reentry_limit(&mut self) {
        self.fail();
    }
    /// Preserve failure details that remain available at the protected-call boundary.
    fn observe_failure(&mut self, _: &Lua, _: usize) {}
    /// Record an invalid `pcall` invocation when the execution context requires it.
    fn invalid_call(&mut self, _: &Lua) {}
    /// Record a caught failure without reclassifying an already captured interruption.
    fn caught(&mut self) {}
    /// Preserve allocator failure while leaving other classification to the context.
    fn caught_error(&mut self, error: &Error) {
        if matches!(error, Error::MemoryError(_)) {
            self.fail();
        }
        self.caught();
    }
}

pub(crate) const REENTRY_LIMIT: usize = 16;

// Track re-entry without holding a `RefCell` borrow across Lua. Drop restores
// the depth on both success and error paths.
struct Reentry<'a>(&'a Cell<usize>);

impl Drop for Reentry<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

pub(crate) fn install<S: State + 'static>(lua: &Lua, state: &Rc<RefCell<S>>) -> luaur::Result<()> {
    install_with_recoverable(lua, state, Rc::new(Cell::new(false)))
}

pub(crate) fn install_with_recoverable<S: State + 'static>(
    lua: &Lua,
    state: &Rc<RefCell<S>>,
    recoverable: Rc<Cell<bool>>,
) -> luaur::Result<()> {
    install_error(lua, state)?;
    let state = state.clone();
    let depth = Cell::new(0);
    // Keep the error text in the registry; capturing a Lua handle in the
    // callback would create a VM-to-closure-to-VM reference cycle.
    lua.set_named_registry_value("mock.protected.error", "protected call failed")?;
    let pcall = lua.create_function(move |lua, mut arguments: MultiValue| {
        state.borrow_mut().observe_line(lua, 1);
        let Some(Value::Function(function)) = arguments.pop_front() else {
            state.borrow_mut().invalid_call(lua);
            state.borrow_mut().caught();
            return failure(lua);
        };
        if depth.get() >= REENTRY_LIMIT {
            // VM interruption and allocation limits do not bound the Rust stack.
            state.borrow_mut().reentry_limit();
            state.borrow_mut().observe_failure(lua, 1);
            return Err(Error::runtime("protected call reentry limit exceeded"));
        }
        depth.set(depth.get() + 1);
        let _entry = Reentry(&depth);
        // Standard `pcall` would hide allocator failure inside Lua. Observe it
        // in Rust so the request cannot commit.
        match function.call::<MultiValue>(arguments) {
            Ok(mut values) => {
                values.push_front(Value::Boolean(true));
                Ok(values)
            }
            Err(error) => {
                if take_recoverable(&recoverable, &error) {
                    state.borrow_mut().caught();
                } else {
                    state.borrow_mut().caught_error(&error);
                }
                failure(lua)
            }
        }
    })?;
    lua.globals().set("pcall", pcall)
}

fn take_recoverable(recoverable: &Cell<bool>, error: &Error) -> bool {
    let marked = recoverable.replace(false);
    marked && matches!(error, Error::RuntimeError(_))
}

fn failure(lua: &Lua) -> luaur::Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![
        Value::Boolean(false),
        lua.named_registry_value("mock.protected.error")?,
    ]))
}

fn install_error<S: State + 'static>(lua: &Lua, state: &Rc<RefCell<S>>) -> luaur::Result<()> {
    let original: luaur::Function = lua.globals().get("error")?;
    lua.set_named_registry_value("mock.explicit.error", original)?;
    let state = state.clone();
    let error = lua.create_function(move |lua, arguments: MultiValue| {
        state.borrow_mut().explicit_error();
        // Retrieve the original from the registry instead of capturing it and
        // creating a VM-to-closure-to-VM reference cycle.
        let original: luaur::Function = lua.named_registry_value("mock.explicit.error")?;
        original.call::<MultiValue>(arguments)
    })?;
    lua.globals().set("error", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoverable_marker_never_hides_allocator_error() {
        let marker = Cell::new(true);
        assert!(!take_recoverable(
            &marker,
            &Error::MemoryError("allocation failed".into())
        ));
        assert!(!marker.get());
        marker.set(true);
        assert!(take_recoverable(
            &marker,
            &Error::RuntimeError("native error".into())
        ));
    }
}
