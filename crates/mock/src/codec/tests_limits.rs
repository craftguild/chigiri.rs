//! Verify that VM strings are reserved before conversion into owned Rust values.

use chigiri::core::Value;
use luaur::{Table, Value as LuaValue};

use super::tests::{setup, ty};
use crate::value::MAX_NODES;

#[test]
fn rsm_16_codec_checks_depth_lists_and_strings_on_both_directions() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    for (length, valid) in [(1000, true), (1001, false)] {
        let (_, mut codec, _) = setup();
        let raw = Value::List(vec![Value::Null; length]);
        assert_eq!(codec.encode(&raw).is_ok(), valid);
        let (lua, mut codec, ir) = setup();
        let table = lua.create_table();
        for index in 1..=length {
            table.raw_set(index, codec.null()).unwrap();
        }
        assert_eq!(
            codec
                .decode(&ir, ty(&ir, "list"), &LuaValue::Table(table))
                .is_ok(),
            valid
        );
    }
    for (bytes, valid) in [(4096, true), (4097, false)] {
        let (lua, mut codec, ir) = setup();
        let value = LuaValue::String(lua.create_string("x".repeat(bytes)));
        assert_eq!(codec.decode_json(&ir, &value).is_ok(), valid);
        if !valid {
            assert_eq!(codec.budget.bytes(), 0);
        }
        let (_, mut codec, _) = setup();
        assert_eq!(
            codec.encode(&Value::String("x".repeat(bytes))).is_ok(),
            valid
        );
    }
    for (depth, valid) in [(32, true), (33, false)] {
        let (lua, mut codec, ir) = setup();
        let mut raw = Value::Null;
        let mut vm = codec.null();
        for _ in 1..depth {
            raw = Value::List(vec![raw]);
            let table = lua.create_table();
            table.raw_set(1, vm).unwrap();
            vm = LuaValue::Table(table);
        }
        assert_eq!(codec.decode_json(&ir, &vm).is_ok(), valid);
        let mut codec = super::Codec::new(&lua).unwrap();
        assert_eq!(codec.encode(&raw).is_ok(), valid);
    }
}

#[test]
fn rsm_16_codec_counts_repeated_nulls_and_key_bytes_before_copy() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (_, mut codec, ir) = setup();
    for _ in 0..MAX_NODES {
        codec.decode_json(&ir, &codec.null()).unwrap();
    }
    assert_eq!(codec.budget.nodes(), MAX_NODES);
    assert!(codec.decode_json(&ir, &codec.null()).is_err());
    assert!(codec.encode(&Value::Null).is_err());
    let (lua, mut codec, ir) = setup();
    assert!(codec.budget.key_bytes(crate::api::WORK_LIMIT).is_err());
    assert_eq!(
        codec.budget.bytes(),
        0,
        "a failed reservation must not copy the key"
    );
    let table: Table = lua.load("return {key = 'x'}").eval().unwrap();
    assert!(
        codec
            .decode_json(&ir, &LuaValue::Table(table.clone()))
            .is_err()
    );
    assert!(codec.decode_json(&ir, &codec.null()).is_err());
    let mut recovered = super::Codec::new(&lua).unwrap();
    let bound = recovered.scope().table(&table).unwrap();
    assert_eq!(
        recovered
            .decode_json(&ir, &bound)
            .unwrap()
            .as_object()
            .unwrap()
            .get("key"),
        Some(&Value::from("x"))
    );
}

#[test]
fn rsm_16_codec_rejects_giant_vm_strings_and_keys_without_rust_copy() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    for source in [
        "return string.rep('private', 100000)",
        "return {[string.rep('private', 100000)] = 1}",
    ] {
        let (lua, mut codec, ir) = setup();
        let raw: LuaValue = lua.load(source).eval().unwrap();
        assert_eq!(
            codec.decode_json(&ir, &raw).unwrap_err().to_string(),
            "string or key length"
        );
        assert!(codec.budget.bytes() <= 2);
    }
}

#[test]
fn rsm_16_codec_keeps_long_schema_names_and_typename_in_budget() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let long = "X".repeat(5000);
    let source = format!("type {long} {{ id: ID! }} union R = {long} type Query {{ r: R }}");
    let ir = std::sync::Arc::new(
        crate::ir::MockIr::parse(vec![chigiri::core::Source::new("long.graphql", source)]).unwrap(),
    );
    let mut seed = chigiri::core::Object::new();
    seed.insert(
        &long,
        Value::List(vec![Value::Object([("id", "i")].into_iter().collect())]),
    );
    let store = crate::store::Store::seeded(ir.clone(), &seed).unwrap();
    let model = ir.model_for(ir.graph().type_id(&long).unwrap()).unwrap();
    let lua = luaur::Lua::new();
    let mut codec = super::Codec::new(&lua).unwrap();
    let snapshot = store.snapshot();
    let encoded = codec
        .encode_record(&ir, snapshot.record(model, "i").unwrap())
        .unwrap();
    let decoded = codec.decode(&ir, ty(&ir, "r"), &encoded).unwrap();
    assert_eq!(
        decoded
            .as_object()
            .unwrap()
            .get("__typename")
            .unwrap()
            .as_str(),
        Some(long.as_str())
    );
    assert!(codec.budget.bytes() >= 3 * long.len());
}

#[test]
fn rsm_15_codec_allocation_failure_is_sticky_and_next_vm_recovers() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, _) = setup();
    lua.set_memory_limit(lua.used_memory() + 32).unwrap();
    assert!(codec.encode(&Value::String("x".repeat(4096))).is_err());
    assert!(codec.check().is_err());
    assert!(codec.encode(&Value::Null).is_err());
    drop(codec);
    drop(lua);
    let (_, mut codec, ir) = setup();
    let encoded = codec.encode(&Value::String("next".into())).unwrap();
    assert_eq!(
        codec.decode_json(&ir, &encoded).unwrap(),
        Value::String("next".into())
    );
}

#[test]
fn rsm_15_codec_scans_shared_sparse_table_capacity_once_per_conversion() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    let empty: Table = lua.load("return table.create(100000)").eval().unwrap();
    codec.mark(&empty, super::JsonShape::Object).unwrap();
    let row = lua.create_table();
    for index in 1..=100 {
        row.raw_set(index, empty.clone()).unwrap();
    }
    let outer = lua.create_table();
    for index in 1..=100 {
        outer.raw_set(index, row.clone()).unwrap();
    }
    let raw = LuaValue::Table(outer.clone());
    let mut scan = super::scan::Scan::default();
    let result = codec
        .decode_value(&ir, super::hint::Hint::Json(None), &raw, 1, &mut scan)
        .unwrap();
    assert_eq!(scan.tables(), 3);
    assert_eq!(result.as_list().unwrap().len(), 100);
    assert_eq!(codec.budget.nodes(), 10302);
    // A later host call uses a fresh scan and therefore observes the intervening Luau edit.
    empty.raw_set("added", 42).unwrap();
    let rebound = codec.scope().table(&outer).unwrap();
    let next = codec.0.decode_json(&ir, &rebound).unwrap();
    let item = &next.as_list().unwrap()[0].as_list().unwrap()[0];
    assert_eq!(
        item.as_object().unwrap().get("added"),
        Some(&Value::Int(42))
    );
}

#[test]
fn rsm_14_codec_does_not_treat_json_typename_keys_as_schema_metadata() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (_, mut codec, _) = setup();
    let raw = Value::Object(
        [("__typename", Value::String("x".repeat(5000)))]
            .into_iter()
            .collect(),
    );
    assert!(codec.encode(&raw).is_err());
    assert!(codec.check().is_err());
}
