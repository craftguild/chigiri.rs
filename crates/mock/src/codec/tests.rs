//! Round-trip real Luau values and verify nulls, container shapes, numeric precision, and record typing.

use chigiri::core::Value;
use luaur::Value as LuaValue;

use super::{Codec, JsonShape};
use crate::store::Store;
use crate::value::{ValueBudget, ValueValidator, equal, exact_number};

pub(super) use super::tests_support::{ir, setup, ty};

#[test]
fn rsm_14_codec_preserves_explicit_null_empty_shapes_and_missing_fields() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    let raw = Value::Object(
        [
            ("null", Value::Null),
            ("array", Value::List(vec![])),
            ("object", Value::Object(Default::default())),
            ("items", Value::List(vec![Value::Null, true.into()])),
        ]
        .into_iter()
        .collect(),
    );
    let encoded = codec.encode(&raw).unwrap();
    let decoded = codec.decode_json(&ir, &encoded).unwrap();
    assert!(equal(&raw, &decoded));
    let field = ir.graph().types()[ir.graph().query().0].fields["read"];
    let input = ir.graph().fields()[field.0].arguments[0];
    let input_ty = &ir.graph().inputs()[input.0].ty;
    let empty = LuaValue::Table(lua.create_table());
    let Value::Object(absent) = codec.decode(&ir, input_ty, &empty).unwrap() else {
        panic!("input required");
    };
    assert!(!absent.contains_key("optional"));
    let table = lua.create_table();
    table.raw_set("optional", codec.null()).unwrap();
    let explicit = codec
        .decode(&ir, input_ty, &LuaValue::Table(table))
        .unwrap();
    assert!(
        explicit
            .as_object()
            .unwrap()
            .get("optional")
            .unwrap()
            .is_null()
    );
    ValueValidator::new(&ir, &mut ValueBudget::default())
        .input(input, &explicit)
        .unwrap();
    assert!(codec.decode_json(&ir, &LuaValue::Nil).is_err());
    assert!(codec.encode(&Value::Null).is_err());
}

#[test]
fn rsm_14_codec_infers_typed_empty_container_and_requires_ambiguous_json_marker() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    for (field, array) in [("map", false), ("array", true), ("list", true)] {
        let raw = LuaValue::Table(lua.create_table());
        let decoded = codec.decode(&ir, ty(&ir, field), &raw).unwrap();
        assert_eq!(matches!(decoded, Value::List(_)), array);
    }
    for (shape, array) in [(JsonShape::Object, false), (JsonShape::Array, true)] {
        let table = lua.create_table();
        codec.mark(&table, shape).unwrap();
        let decoded = codec.decode_json(&ir, &LuaValue::Table(table)).unwrap();
        assert_eq!(matches!(decoded, Value::List(_)), array);
    }
    assert!(
        codec
            .decode_json(&ir, &LuaValue::Table(lua.create_table()))
            .is_err()
    );
}

#[test]
fn rsm_14_codec_rejects_cycles_holes_mixed_keys_and_foreign_values() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    for source in [
        "local t = {}; t.self = t; return t",
        "return {[1] = 1, [3] = 3}",
        "return {[1] = 1, named = 2}",
        "return {[0] = 1}",
        "return {[-1] = 1}",
        "return {[1.5] = 1}",
        "return {[101] = 1}",
        "return {[true] = 1}",
        "return {data = function() end}",
        "return setmetatable({}, {__index = function() error('must not run') end})",
        "return 0/0",
        "return 1/0",
    ] {
        let (lua, mut codec, ir) = setup();
        let value: LuaValue = lua.load(source).eval().unwrap();
        assert!(codec.decode_json(&ir, &value).is_err(), "{source}");
        assert!(codec.decode_json(&ir, &codec.null()).is_err());
    }
    let (lua, mut codec, ir) = setup();
    let foreign = Codec::new(&lua).unwrap().null();
    assert!(codec.decode_json(&ir, &foreign).is_err());
    let (_, mut codec, ir) = setup();
    let invalid = LuaValue::String(codec.lua.create_string([255]));
    let message = codec.decode_json(&ir, &invalid).unwrap_err().to_string();
    assert_eq!(message, "VM value conversion failed");
}

#[test]
fn rsm_14_codec_allows_shared_acyclic_tables_but_checks_shape_conflicts() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    let shared: LuaValue = lua
        .load("local t = {value = 1}; return {a = t, b = t}")
        .eval()
        .unwrap();
    let decoded = codec.decode_json(&ir, &shared).unwrap();
    let object = decoded.as_object().unwrap();
    assert!(equal(object.get("a").unwrap(), object.get("b").unwrap()));
    let table = lua.create_table();
    codec.mark(&table, JsonShape::Object).unwrap();
    assert!(
        codec
            .decode(&ir, ty(&ir, "list"), &LuaValue::Table(table))
            .is_err()
    );
    let (lua, mut codec, _) = setup();
    let table = lua.create_table();
    codec.mark(&table, JsonShape::Array).unwrap();
    assert!(codec.mark(&table, JsonShape::Object).is_err());
}

#[test]
fn rsm_09_codec_preserves_exact_numbers_including_luaur_i64_boundary() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    for number in [
        -(2.0f64).powi(127),
        -(2.0f64).powi(63),
        -0.0,
        0.25,
        (2.0f64).powi(53) + 2.0,
        (2.0f64).powi(63),
        (2.0f64).powi(127),
        (2.0f64).powi(200),
        f64::MAX,
    ] {
        let original = Value::Float(number);
        let encoded = codec.encode(&original).unwrap();
        let identity: luaur::Function = lua.load("return function(x) return x end").eval().unwrap();
        let vm: LuaValue = identity.call(encoded.as_lua().clone()).unwrap();
        let decoded = codec.decode(&ir, ty(&ir, "json"), &vm).unwrap();
        assert_eq!(exact_number(&decoded).unwrap(), number);
    }
    let vm: LuaValue = lua.load("return 9223372036854775808").eval().unwrap();
    assert!(matches!(vm, LuaValue::Integer(i64::MAX)));
    assert_eq!(
        codec.decode(&ir, ty(&ir, "integer"), &vm).unwrap(),
        Value::Int(1i128 << 63)
    );
    for invalid in [
        Value::Int((1i128 << 53) + 1),
        Value::Int(i128::MAX),
        Value::Float(f64::INFINITY),
    ] {
        let (_, mut codec, _) = setup();
        assert!(codec.encode(&invalid).is_err());
    }
}

#[test]
fn rsm_07_codec_uses_storage_slots_and_concrete_typename_for_abstract_return() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    let store = Store::from_seed_json(
        ir.clone(),
        r#"{"Device":[{"id":"d","name":"device","ownerId":null}]}"#,
    )
    .unwrap();
    let snapshot = store.snapshot();
    let model = ir.model_for(ir.graph().type_id("Device").unwrap()).unwrap();
    let record = snapshot.record(model, "d").unwrap();
    let encoded = codec.encode_record(&ir, record).unwrap();
    let decoded = codec.decode(&ir, ty(&ir, "result"), &encoded).unwrap();
    let object = decoded.as_object().unwrap();
    assert_eq!(object.get("__typename").unwrap().as_str(), Some("Device"));
    assert!(object.contains_key("ownerId"));
    assert!(!object.contains_key("owner"));
    let field = ir.graph().types()[ir.graph().query().0].fields["result"];
    snapshot.reader().output(field, &decoded).unwrap();
    let wrong: LuaValue = lua
        .load("return {__typename = 'Input', optional = 'x'}")
        .eval()
        .unwrap();
    assert!(codec.decode(&ir, ty(&ir, "result"), &wrong).is_err());
}

#[test]
fn sa_05_record_provenance_pins_the_original_vm_table_across_collection() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, mut codec, ir) = setup();
    let store = Store::from_seed_json(
        ir.clone(),
        r#"{"Device":[{"id":"d","name":"device","ownerId":null}]}"#,
    )
    .unwrap();
    let model = ir.model_for(ir.graph().type_id("Device").unwrap()).unwrap();
    let snapshot = store.snapshot();
    let encoded = codec
        .encode_record(&ir, snapshot.record(model, "d").unwrap())
        .unwrap();
    let LuaValue::Table(table) = encoded.as_lua() else {
        panic!("record table required");
    };
    let pointer = table.to_pointer() as usize;
    drop(encoded);
    lua.gc_collect().unwrap();
    let (pinned, record) = codec.records.get(&pointer).unwrap();
    assert_eq!(pinned.to_pointer() as usize, pointer);
    let super::RecordOrigin::Live(record) = record else {
        panic!("live record required")
    };
    assert_eq!(record.id(), Some("d"));
}

#[test]
fn sa_05_codec_rejects_record_from_another_contract() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (_, mut codec, ir) = setup();
    let other = super::tests::ir();
    let store =
        Store::from_seed_json(other.clone(), r#"{"Device":[{"id":"d","name":"device"}]}"#).unwrap();
    let model = other
        .model_for(other.graph().type_id("Device").unwrap())
        .unwrap();
    let snapshot = store.snapshot();
    let record = snapshot.record(model, "d").unwrap();
    assert_eq!(
        codec.encode_record(&ir, record).err().unwrap().to_string(),
        "foreign schema record"
    );
    assert!(codec.encode_record(&other, record).is_err());
}

#[test]
fn rsm_01_codec_rejects_foreign_vm_handles_before_stack_operations() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (a, mut codec_a, ir) = setup();
    let (b, mut codec_b, _) = setup();
    let foreign = codec_b.encode(&Value::String("foreign".into())).unwrap();
    assert!(codec_a.0.decode_json(&ir, &foreign).is_err());
    let codec_a = Codec::new(&a).unwrap();
    let foreign_table = b.create_table();
    assert!(codec_a.scope().table(&foreign_table).is_err());
    assert!(codec_a.check().is_err());
    let mut codec_a = Codec::new(&a).unwrap();
    assert!(codec_a.mark(&foreign_table, JsonShape::Object).is_err());
    let codec_a = Codec::new(&a).unwrap();
    let foreign_function: luaur::Function = b
        .load("return function() return 'foreign' end")
        .eval()
        .unwrap();
    assert!(codec_a.scope().call(&foreign_function, &[]).is_err());
    let codec_a = Codec::new(&a).unwrap();
    let own: luaur::Function = a
        .load("return function(value) return value end")
        .eval()
        .unwrap();
    assert!(codec_a.scope().call(&own, &[foreign]).is_err());
    assert!(codec_a.check().is_err());
    let codec_a = Codec::new(&a).unwrap();
    let result = codec_a.scope().call(&own, &[codec_a.null()]).unwrap();
    let mut codec_a = codec_a;
    assert!(codec_a.decode_json(&ir, &result).unwrap().is_null());
    assert_eq!(a.load("return 42").eval::<i64>().unwrap(), 42);
    assert_eq!(b.load("return 43").eval::<i64>().unwrap(), 43);
}

#[test]
fn rsm_15_codec_failure_and_drop_invalidate_previously_cloned_scopes() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let (lua, codec, ir) = setup();
    let mut codec = codec.0;
    let scope = codec.scope();
    let function: luaur::Function = lua.load("return function() return 42 end").eval().unwrap();
    assert!(
        codec
            .decode_record(&ir, crate::ir::ModelId(usize::MAX), &codec.null())
            .is_err()
    );
    assert!(scope.call(&function, &[]).is_err());
    assert!(scope.table(&lua.create_table()).is_err());
    let codec = Codec::new(&lua).unwrap();
    let scope = codec.scope();
    drop(codec);
    assert!(scope.call(&function, &[]).is_err());
    assert!(scope.table(&lua.create_table()).is_err());
    let codec = Codec::new(&lua).unwrap();
    let scope = codec.scope();
    let fail = scope.clone();
    let callback = lua
        .create_function(move |_, ()| {
            fail.fail();
            Ok(42)
        })
        .unwrap();
    assert!(scope.call(&callback, &[]).is_err());
    assert!(codec.check().is_err());
}
