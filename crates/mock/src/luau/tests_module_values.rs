//! Verify that the Rust module boundary rejects metatables and non-UTF-8 values unavailable to normal sandbox code.

use luaur::{Lua, Value};

use super::tests::contract;
use super::{ScriptCandidate, module_value};
use crate::ir::ScriptId;

#[test]
fn mp_21_module_decoder_rejects_metatable_invalid_strings_and_large_keys_before_copy() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let script = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source: "return {resolve=function(): number return 1 end}".into(),
        })
        .unwrap();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    lua.set_memory_limit(crate::value::MAX_BYTES).unwrap();
    let length = crate::sandbox::string_length(&lua).unwrap();
    let function: luaur::Function = lua.load("return function() return 1 end").eval().unwrap();
    let table = lua.create_table_result().unwrap();
    table.raw_set("resolve", function).unwrap();
    assert!(module_value::decode(Value::Table(table.clone()), &length, &script).is_ok());
    table
        .set_metatable(Some(lua.create_table_result().unwrap()))
        .unwrap();
    assert!(module_value::decode(Value::Table(table.clone()), &length, &script).is_err());
    table.set_metatable(None).unwrap();
    table.raw_set(lua.create_string([0xff]), true).unwrap();
    assert!(module_value::decode(Value::Table(table.clone()), &length, &script).is_err());
    let table = lua.create_table_result().unwrap();
    let function: luaur::Function = lua.load("return function() return 1 end").eval().unwrap();
    table.raw_set("resolve", function).unwrap();
    table
        .raw_set(lua.create_string("x".repeat(1024 * 1024)), true)
        .unwrap();
    assert!(module_value::decode(Value::Table(table), &length, &script).is_err());
}

#[test]
fn cv_03_closed_shape_is_revalidated_on_every_module_load() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let script = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source: "local globals = _G :: any\nlocal module: any = {resolve=function(): number return 1 end}\nif globals.seen then module.state = 'ready' end\nglobals.seen = true\nreturn module".into(),
        })
        .unwrap();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let length = crate::sandbox::string_length(&lua).unwrap();
    crate::sandbox::seal(&lua).unwrap();
    module_value::load(&lua, &length, &script).unwrap();
    let error = module_value::load(&lua, &length, &script).err().unwrap();
    assert_eq!(error.phase, "module");
}

#[test]
fn sa_02_caught_native_startup_work_is_cumulative_and_sticky() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let text = "x".repeat(crate::value::MAX_STRING_BYTES);
    let source = format!(
        "local value='{text}'\nfor _=1,300 do pcall(function() return string.lower(value) end) end\nreturn {{resolve=function(): number return 1 end}}"
    );
    let error = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source,
        })
        .err()
        .expect("caught native work limit must reject initialization");
    assert_eq!(error.phase, "limit");
}
