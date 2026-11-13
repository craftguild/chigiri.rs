//! Verify the evaluator owns a closed, explicitly typed settings value.

use super::evaluate_config;

pub(super) const CONFIG: &str = r#"
return {
    hooks = {request = {before = false, after = false}, operation = {before = false, after = false}, field = {before = false, after = false}},
    state = {fields = {}, initial = {}},
    format = "chigiri-mock-project/v10",
    schema = {sources = {"../schema.graphql"}},
    storage = {sources = {}},
    clock = {mode = "system"},
    server = {listen = "127.0.0.1:0", allowed_origins = {}},
}
"#;

#[test]
fn mp_04_computed_config_becomes_owned_and_server_rules_are_preserved() {
    let _case = crate::vm_gate::test_case();
    let source = format!(
        "local prefix = '../'\n{}",
        CONFIG.replace("\"../schema.graphql\"", "prefix .. 'schema.graphql'")
    );
    let settings = evaluate_config(&source).unwrap();
    assert_eq!(settings.sources, ["../schema.graphql"]);
    assert_eq!(settings.server.listen.to_string(), "127.0.0.1:0");
    assert!(settings.server.allowed_origins.is_empty());
    drop(source);
    // Returned Rust strings remain owned after the source and VM are dropped and across later VM use.
    assert_eq!(evaluate_config(CONFIG).unwrap().sources, settings.sources);
    for address in ["0.0.0.0:4000", "localhost:4000", "192.0.2.1:4000"] {
        assert!(evaluate_config(&CONFIG.replace("127.0.0.1:0", address)).is_err());
    }
    assert!(evaluate_config(&CONFIG.replace("127.0.0.1:0", "[::1]:0")).is_ok());
    for origin in ["http://localhost:5173", "https://example.test"] {
        let source = CONFIG.replace(
            "allowed_origins = {}",
            &format!("allowed_origins = {{'{origin}'}}"),
        );
        assert_eq!(
            evaluate_config(&source).unwrap().server.allowed_origins,
            [origin]
        );
    }
    for origin in [
        "*",
        "http://localhost/",
        "http://x@y",
        "file:///tmp",
        "https://x?q=1",
    ] {
        let source = CONFIG.replace(
            "allowed_origins = {}",
            &format!("allowed_origins = {{'{origin}'}}"),
        );
        assert!(evaluate_config(&source).is_err());
    }
}

#[test]
fn ev_12_configuration_accepts_only_the_current_format_without_update_exceptions() {
    let _case = crate::vm_gate::test_case();
    assert!(evaluate_config(CONFIG).is_ok());
    let legacy = CONFIG.replace("chigiri-mock-project/v10", "chigiri-mock-project/v8");
    assert!(evaluate_config(&legacy).is_err());
    assert!(
        evaluate_config(&CONFIG.replace("chigiri-mock-project/v10", "chigiri-mock-project/v7"))
            .is_err()
    );
    let malformed_current = CONFIG.replace(
        "format = \"chigiri-mock-project/v10\"",
        "format = \"chigiri-mock-project/v10\", extra = true",
    );
    assert!(evaluate_config(&malformed_current).is_err());
}

#[test]
fn tl_02_fixed_clock_requires_an_admitted_integer_unix_second() {
    let _case = crate::vm_gate::test_case();
    let fixed = CONFIG.replace(
        "clock = {mode = \"system\"}",
        "clock = {mode = \"fixed\", now = 1789776000}",
    );
    assert_eq!(
        evaluate_config(&fixed).unwrap().clock.fixed_value(),
        Some(1789776000)
    );
    for invalid in [
        "clock = {mode = \"fixed\", now = \"2026-09-19T00:00:00.000Z\"}",
        "clock = {mode = \"fixed\", now = 1789776000.5}",
        "clock = {mode = \"system\", now = 1789776000}",
    ] {
        let source = CONFIG.replace("clock = {mode = \"system\"}", invalid);
        assert!(evaluate_config(&source).is_err(), "{invalid}");
    }
}

#[test]
fn mp_04_rust_rejects_unknown_function_cycle_sparse_and_wrong_shapes_after_any() {
    let _case = crate::vm_gate::test_case();
    let base = CONFIG.replace("return {", "local config: any = {");
    for edit in [
        "config.extra = 1",
        "config.schema.extra = 1",
        "config.server.extra = 1",
        "config.format = 'chigiri-mock-project/v2'",
        "config.schema.sources = {}",
        "config.schema.sources = {[1]='a', [3]='b'}",
        "config.schema.sources = {a='b'}",
        "config.schema.sources = {[0]='a'}",
        "config.schema.sources = {[1.5]='a'}",
        "config.schema.sources = {function() return 'a' end}",
        "config.schema.sources = {config}",
        "config.server.allowed_origins = {config}",
        "config.schema = config",
        "config.server.listen = 7",
        "config.schema.sources = {'https://example.test/a'}",
        "config.schema.sources = {'*.graphql'}",
        "config.schema.sources = {'a\\000b'}",
    ] {
        let source = format!("{base}\n{edit}\nreturn config");
        let result = evaluate_config(&source);
        assert!(result.is_err(), "{edit}");
        assert_ne!(
            result.err().unwrap().phase,
            "type",
            "fixture must reach Rust validation: {edit}"
        );
    }
}

#[test]
fn mp_04_metatable_and_non_utf8_are_rejected_by_rust_decoder() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let strings: luaur::Table = lua.globals().get("string").unwrap();
    let length: luaur::Function = strings.get("len").unwrap();
    let table: luaur::Table = lua.load(CONFIG).eval().unwrap();
    table.set_metatable(Some(lua.create_table())).unwrap();
    assert!(
        super::config_decode::decode(
            &lua,
            luaur::Value::Table(table),
            length.clone(),
            super::FORMAT,
        )
        .is_err()
    );
    let table: luaur::Table = lua.load(CONFIG).eval().unwrap();
    table.raw_set("format", lua.create_string([0xff])).unwrap();
    assert!(
        super::config_decode::decode(&lua, luaur::Value::Table(table), length, super::FORMAT,)
            .is_err()
    );
}

#[test]
fn mp_05_config_has_only_minimum_globals_and_no_string_metatable() {
    let _case = crate::vm_gate::test_case();
    let source = format!(
        r#"
local expected: {{[string]: boolean}} = {{assert=true,error=true,pcall=true,type=true,pairs=true,ipairs=true,tostring=true,tonumber=true,string=true,math=true,table=true,os=true,timeleap=true,_G=true}}
local count = 0
for key in pairs(_G) do assert(expected[key]) count += 1 end
assert(count == 14)
assert(os.time ~= nil and os.date ~= nil and os.difftime ~= nil and os.clock ~= nil)
local dynamicOs = os :: any
assert(dynamicOs.execute == nil and dynamicOs.getenv == nil)
assert(('abc'):sub(2) == 'bc')
assert(('x').rep == nil)
{CONFIG}
"#
    );
    if let Err(error) = evaluate_config(&source) {
        panic!("capability config failed: {error:?}");
    }
}

#[test]
fn mp_04_31_config_type_syntax_source_and_source_count_boundaries() {
    let _case = crate::vm_gate::test_case();
    assert_eq!(evaluate_config("return 1").err().unwrap().phase, "type");
    assert_eq!(
        evaluate_config("end return {} ").err().unwrap().phase,
        "compile"
    );
    let source = format!(
        "--{}\n{CONFIG}",
        "x".repeat(crate::limits::CONFIG_BYTES - CONFIG.len() - 3)
    );
    assert_eq!(source.len(), crate::limits::CONFIG_BYTES);
    assert!(evaluate_config(&source).is_ok());
    assert_eq!(
        evaluate_config(&(source + " ")).err().unwrap().phase,
        "limit"
    );
    for count in [64, 65] {
        let paths = vec!["'schema.graphql'"; count].join(",");
        let source = CONFIG.replace("\"../schema.graphql\"", &paths);
        assert_eq!(evaluate_config(&source).is_ok(), count == 64);
    }
}
