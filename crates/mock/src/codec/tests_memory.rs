//! Convert allocator failures observed during `Function::call` into `ValueError` without unwinding.

use super::*;
use crate::limits::request::{BudgetFailure, LocalLimit};

#[test]
fn vm_string_length_allocation_failure_is_reported_as_memory_failure() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let work = crate::value::work::Work::default();
    let mut codec = Codec::new_with_work(&lua, work.clone()).unwrap();
    // Replace string length with an allocation-heavy callback that fails inside `Function::call`.
    codec.string_len = lua
        .load("return function(_) local s='x' for _=1,30 do s=s..s end return 0 end")
        .eval()
        .unwrap();
    let text = lua.create_string("small");
    let ir = crate::ir::MockIr::parse(vec![chigiri::core::Source::new(
        "memory.graphql",
        "type Query{value:String!}",
    )])
    .unwrap();
    let value = codec.scope.wrap(LuaValue::String(text));
    assert!(codec.decode_json(&ir, &value).is_err());
    assert_eq!(
        work.failure(),
        Some(BudgetFailure::LocalHard(LocalLimit::VmMemory))
    );
    assert!(codec.check().is_err());
}

#[test]
fn host_string_bounds_run_before_utf8_copy_and_budget_charge() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let mut codec = Codec::new(&lua).unwrap();
    let invalid = lua.create_string([0xff; 9]);
    let before = (codec.budget.nodes(), codec.budget.bytes());
    let error = codec
        .bounded_host_string(&invalid, 8, HostStringKind::Value)
        .unwrap_err();
    assert_eq!(error, ValueError("string or key length"));
    assert_eq!((codec.budget.nodes(), codec.budget.bytes()), before);

    let invalid = lua.create_string([0xff; 8]);
    assert!(
        codec
            .bounded_host_string(&invalid, 8, HostStringKind::Value)
            .is_err()
    );
    assert_eq!(codec.budget.nodes(), before.0 + 1);
    assert_eq!(codec.budget.bytes(), before.1 + 8);
}

#[test]
fn host_string_limits_count_utf8_bytes_for_values_and_keys() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let mut codec = Codec::new(&lua).unwrap();
    let exact = lua.create_string("éééé");
    assert_eq!(
        codec
            .bounded_host_string(&exact, 8, HostStringKind::Value)
            .unwrap(),
        "éééé"
    );
    let over = lua.create_string("ééééx");
    assert_eq!(
        codec
            .bounded_host_string(&over, 8, HostStringKind::Key)
            .unwrap_err(),
        ValueError("string or key length")
    );
}
