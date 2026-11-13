//! Source userdata tokens are owner-bound and one-shot without Lua table identity.

use super::binding::BindingCollector;
use crate::ir::ScriptId;

#[test]
fn ev_01_tokens_reject_foreign_owners_fields_forgery_and_reuse() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let first = BindingCollector::new(ScriptId(0));
    let same_field = BindingCollector::new(ScriptId(0));
    let other_field = BindingCollector::new(ScriptId(1));
    let token = first
        .push(
            &lua,
            "selected".into(),
            chigiri::SubscriptionOverflow::Backpressure,
        )
        .unwrap();
    assert!(same_field.take(&token).is_err());
    assert!(other_field.take(&token).is_err());
    assert!(
        first
            .take(&luaur::Value::Table(lua.create_table()))
            .is_err()
    );
    let binding = first.take(&token).unwrap();
    assert_eq!(binding.script(), ScriptId(0));
    assert_eq!(binding.topic().unwrap(), "selected");
    assert!(first.take(&token).is_err());
    // A token contains no callback, collector or VM reference. Keeping it in
    // this local VM does not keep the Rust candidate collector open.
    assert!(first.closed());
    assert!(same_field.closed());
    assert!(other_field.closed());
}

#[test]
fn ev_01_source_allocation_failure_is_contained_and_fresh_vm_recovers() {
    let _case = crate::vm_gate::test_case();
    let lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let collector = BindingCollector::new(ScriptId(0));
    let entered = std::rc::Rc::new(std::cell::Cell::new(false));
    let invoked = entered.clone();
    let pending = collector.clone();
    let factory = lua
        .create_function(move |lua, ()| {
            invoked.set(true);
            // Small userdata can consume existing VM pages without calling
            // the allocator. Retain a bounded batch to force a new page.
            let mut tokens = Vec::new();
            for _ in 0..1_024 {
                tokens.push(pending.push(
                    lua,
                    "bounded".into(),
                    chigiri::SubscriptionOverflow::Backpressure,
                )?);
            }
            Ok(tokens.pop().unwrap())
        })
        .unwrap();
    lua.gc_collect().unwrap();
    lua.set_memory_limit(lua.used_memory()).unwrap();
    // The released VM's allocation helpers can raise a native exception. Use
    // the same protected function-call boundary as a real setup invocation.
    let result = factory.call::<luaur::Value>(());
    let failure_class = match &result {
        Err(luaur::Error::MemoryError(_)) => "memory",
        Err(luaur::Error::RuntimeError(_)) => "runtime",
        Err(_) => "other_error",
        Ok(_) => "success",
    };
    drop(result);
    drop(factory);
    drop(lua);
    let closed = collector.closed();
    drop(lease);
    assert!(entered.get());
    // luaur-rt 0.1.8 catches a native allocator exception inside its Rust
    // callback trampoline and returns RuntimeError. Source creation must not
    // turn that failure into a successful handle or unwind this VM lease.
    assert_eq!(failure_class, "runtime");
    assert!(closed);
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let fresh = BindingCollector::new(ScriptId(0));
    let token = fresh
        .push(
            &lua,
            "recovered".into(),
            chigiri::SubscriptionOverflow::Backpressure,
        )
        .unwrap();
    assert_eq!(fresh.take(&token).unwrap().topic().unwrap(), "recovered");
    assert!(fresh.closed());
}
