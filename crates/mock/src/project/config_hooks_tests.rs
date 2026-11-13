//! Validate six-slot hook declarations and initial state after type checking without retaining VM handles or live request values.

use chigiri::core::Value;
use luaur::{Function, Lua, Table, Value as LuaValue};

use super::config_tests::CONFIG;
use super::evaluate_config;
use crate::hooks::{HookSettings, HookSlot, StateType};

fn edited(edit: &str) -> String {
    format!(
        "{}\n{edit}\nreturn config",
        CONFIG.replace("return {", "local config: any = {")
    )
}

#[test]
fn hk_01_config_six_slots_are_explicit_closed_and_disabled_by_default() {
    let _case = crate::vm_gate::test_case();
    let settings = evaluate_config(CONFIG).unwrap();
    assert_eq!(settings.hooks, HookSettings::default());
    assert!(
        HookSlot::ALL
            .iter()
            .all(|slot| !settings.hooks.enabled(*slot))
    );
    assert!(settings.state.fields().is_empty());
    assert!(settings.state.initial().is_empty());
    for slot in HookSlot::ALL {
        let settings =
            evaluate_config(&edited(&format!("config.hooks.{} = true", slot.name()))).unwrap();
        for other in HookSlot::ALL {
            assert_eq!(settings.hooks.enabled(other), slot == other);
        }
    }
    for edit in [
        "config.hooks = nil",
        "config.state = nil",
        "config.hooks.request = nil",
        "config.hooks.request.before = nil",
        "config.hooks.request.before = 1",
        "config.hooks.request.before = 'true'",
        "config.hooks.request.extra = true",
        "config.hooks.extra = {}",
        "config.hooks.field.before = function() end",
        "config.format = 'chigiri-mock-project/v1'",
    ] {
        let error = evaluate_config(&edited(edit)).err().expect(edit);
        assert_ne!(error.phase, "type", "Rust boundary negative: {edit}");
    }
}

#[test]
fn hk_02_config_declared_kinds_preserve_empty_shapes_and_normalize_missing_nullable() {
    let _case = crate::vm_gate::test_case();
    let settings = evaluate_config(&edited(
        r#"
config.state = {
    fields = {
        authenticated = {kind="boolean"}, user_id = {kind="nullable", of={kind="string"}},
        count = {kind="integer"}, ratio = {kind="number"},
        profile = {kind="object", fields={name={kind="string"}}},
        roles = {kind="list", of={kind="string"}}, empty = {kind="object", fields={}},
    },
    initial = {authenticated=false, count=9007199254740991, ratio=0.25,
               profile={name="Users."}, roles={}, empty={}},
}
"#,
    ))
    .unwrap();
    assert_eq!(settings.state.fields()["count"], StateType::Integer);
    assert_eq!(settings.state.initial().get("user_id"), Some(&Value::Null));
    assert_eq!(
        settings.state.initial().get("count"),
        Some(&Value::Int(9_007_199_254_740_991))
    );
    assert_eq!(
        settings.state.initial().get("roles"),
        Some(&Value::List(vec![]))
    );
    assert!(
        settings
            .state
            .initial()
            .get("empty")
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        settings
            .state
            .initial()
            .get("profile")
            .unwrap()
            .as_object()
            .unwrap()
            .get("name")
            .unwrap()
            .as_str(),
        Some("Users.")
    );
    assert!(evaluate_config(CONFIG).unwrap().state.initial().is_empty());
}

#[test]
fn hk_02_rust_rejects_wrong_unknown_cyclic_and_nonfinite_state_after_any() {
    let _case = crate::vm_gate::test_case();
    for edit in [
        "config.state.extra = true",
        "config.state.fields = nil",
        "config.state.initial = nil",
        "config.state.fields.x = {kind='any'}",
        "config.state.fields.x = {kind='string', of={kind='string'}}",
        "config.state.fields.x = {kind='object'}",
        "config.state.fields.x = {kind='list', of={kind='boolean'}, extra=true}",
        "config.state.fields.x = {kind='nullable', of={kind='string'}, fields={}}",
        "config.state.fields.x = {kind='boolean'}",
        "config.state.fields.x = {kind='integer'}; config.state.initial.x = '1'",
        "config.state.fields.x = {kind='integer'}; config.state.initial.x = 1.5",
        "config.state.fields.x = {kind='integer'}; config.state.initial.x = 9007199254740992",
        "config.state.fields.x = {kind='integer'}; config.state.initial.x = -9007199254740992",
        "config.state.fields.x = {kind='number'}; config.state.initial.x = 1/0",
        "config.state.fields.x = {kind='number'}; config.state.initial.x = 0/0",
        "config.state.fields.x = {kind='string'}; config.state.initial.x = function() end",
        "config.state.initial.unknown = 1",
        "config.state.fields.x = {kind='list', of={kind='boolean'}}; config.state.initial.x = {[1]=true,[3]=false}",
        "config.state.fields.x = {kind='list', of={kind='boolean'}}; config.state.initial.x = {[0]=true}",
        "config.state.fields.x = {kind='list', of={kind='boolean'}}; config.state.initial.x = {config.state.initial}",
        "local kind:any = {kind='nullable'}; kind.of=kind; config.state.fields.x=kind",
        "config.state.fields.__private = {kind='nullable',of={kind='boolean'}}",
        "config.state.fields.Name = {kind='nullable',of={kind='boolean'}}; config.state.fields.name = config.state.fields.Name",
    ] {
        let error = evaluate_config(&edited(edit)).err().expect(edit);
        assert_ne!(error.phase, "type", "Rust boundary negative: {edit}");
    }
    // A failure is followed by a fresh VM that shares no settings state.
    assert!(evaluate_config(CONFIG).is_ok());
}

#[test]
fn hk_02_config_state_capacity_is_enforced_before_string_copy_and_list_growth() {
    let _case = crate::vm_gate::test_case();
    for count in [4096, 4097] {
        let source = edited(&format!(
            "config.state.fields.x={{kind='string'}}; config.state.initial.x='{}'",
            "x".repeat(count)
        ));
        assert_eq!(evaluate_config(&source).is_ok(), count == 4096);
    }
    for count in [1000, 1001] {
        let source = edited(&format!(
            "config.state.fields.x={{kind='list',of={{kind='boolean'}}}}; config.state.initial.x={{}}; for i=1,{count} do config.state.initial.x[i]=true end"
        ));
        assert_eq!(
            evaluate_config(&source).is_ok(),
            count == 1000,
            "count={count}"
        );
    }
}

#[test]
fn hk_02_state_rejects_metatables_and_invalid_utf8_at_the_vm_boundary() {
    let _case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let lua = Lua::new();
    let strings: Table = lua.globals().get("string").unwrap();
    let length: Function = strings.get("len").unwrap();
    for edit in [
        "setmetatable(config.state.fields, {})",
        "config.state.fields.x={kind='object',fields={}}; config.state.initial.x=setmetatable({}, {})",
        "config.state.fields.x={kind='string'}; config.state.initial.x=string.char(255)",
    ] {
        let raw = lua.load(edited(edit)).eval::<LuaValue>().unwrap();
        assert!(super::config_decode::decode(&lua, raw, length.clone(), super::FORMAT).is_err());
    }
}

#[test]
fn hk_01_02_config_typechecker_uses_closed_kinds_before_runtime_validation() {
    let _case = crate::vm_gate::test_case();
    let source = CONFIG.replace(
        "state = {fields = {}, initial = {}},",
        r#"state = {
            fields = {
                auth = {kind="boolean"}, count = {kind="integer"}, ratio = {kind="number"},
                user = {kind="nullable", of={kind="string"}},
                names = {kind="list", of={kind="string"}},
                object = {kind="object", fields={name={kind="string"}}},
            },
            initial = {auth=false, count=1, ratio=0.5, names={"résumé 🙂"}, object={name="The value."}},
        },"#,
    );
    let settings = evaluate_config(&source).unwrap();
    assert_eq!(settings.state.initial().get("user"), Some(&Value::Null));
    for change in [
        source.replace("kind=\"boolean\"", "kind=\"unknown\""),
        source.replace("before = false", "before = 1"),
    ] {
        assert_eq!(evaluate_config(&change).err().unwrap().phase, "type");
    }
}
