//! Verifies that portable/IPC JSON has the same seven value kinds, null handling,
//! numeric precision, closed shapes, and shared budgets as configuration input.

use chigiri::core::Value;

use super::StateContract;
use crate::value::{MAX_BYTES, MAX_ITEMS, ValueBudget};

const UNICODE_TEXT: &str = "résumé e\u{301} 🙂";
const UNICODE_SOURCE: &str = r#"résumé e\u0301 🙂"#;

const STATE: &str = r#"{
    "fields": {
        "text":{"kind":"string"}, "flag":{"kind":"boolean"},
        "integer":{"kind":"integer"}, "number":{"kind":"number"},
        "empty":{"kind":"object","fields":{}},
        "items":{"kind":"list","of":{"kind":"string"}},
        "maybe":{"kind":"nullable","of":{"kind":"string"}}
    },
    "initial": {"text":"résumé e\u0301 🙂", "flag":false, "integer":9007199254740991,
        "number":9223372036854775808, "empty":{}, "items":[]}
}"#;

#[test]
fn hk_02_06_json_round_trip_preserves_kinds_explicit_null_empty_shapes_and_exact_numbers() {
    let state = StateContract::from_json(STATE, &mut ValueBudget::default()).unwrap();
    assert_eq!(UNICODE_TEXT.len(), 17);
    assert_eq!(UNICODE_TEXT.chars().count(), 11);
    assert_eq!(
        state.initial().get("text"),
        Some(&Value::String(UNICODE_TEXT.into()))
    );
    assert_eq!(state.initial().get("maybe"), Some(&Value::Null));
    assert_eq!(
        state.initial().get("integer"),
        Some(&Value::Int(9_007_199_254_740_991))
    );
    assert_eq!(
        state.initial().get("number"),
        Some(&Value::Int(1_i128 << 63))
    );
    assert!(matches!(state.initial().get("empty"), Some(Value::Object(value)) if value.is_empty()));
    assert_eq!(state.initial().get("items"), Some(&Value::List(vec![])));
    let json = state.to_json(MAX_BYTES).unwrap();
    assert_eq!(state.to_json(json.len()).unwrap(), json);
    assert!(state.to_json(json.len() - 1).is_err());
    assert!(json.contains(UNICODE_TEXT));
    let second = StateContract::from_json(&json, &mut ValueBudget::default()).unwrap();
    assert_eq!(state.fields(), second.fields());
    assert_eq!(state.initial(), second.initial());
    assert_eq!(serde_json::to_string(&second).unwrap(), json);
}

#[test]
fn hk_02_06_json_rejects_duplicate_unknown_wrong_missing_and_unsafe_values() {
    for source in [
        STATE.replace(
            &format!(r#""text":"{UNICODE_SOURCE}""#),
            &format!(r#""text":"{UNICODE_SOURCE}","text":"duplicate""#),
        ),
        STATE.replace("\"flag\":false", "\"unknown\":false,\"flag\":false"),
        STATE.replace("\"flag\":false,", ""),
        STATE.replace("\"flag\":false", "\"flag\":0"),
        STATE.replace("9007199254740991", "9007199254740992"),
        STATE.replace("9223372036854775808", "1e999999"),
        STATE.replace("\"empty\":{}", "\"empty\":[]"),
        STATE.replace("\"items\":[]", "\"items\":{}"),
        STATE.replace(
            "\"kind\":\"integer\"",
            "\"kind\":\"integer\",\"kind\":\"number\"",
        ),
        STATE.replace("\"kind\":\"integer\"", "\"kind\":\"unrecognized\""),
        STATE.replace(
            "\"kind\":\"integer\"",
            "\"kind\":\"integer\",\"of\":{\"kind\":\"string\"}",
        ),
        "{\"fields\":{},\"initial\":{},\"extra\":true}".into(),
        "{\"fields\":{},\"fields\":{},\"initial\":{}}".into(),
        "{\"fields\":{}}".into(),
        "{\"fields\":{},\"initial\":null}".into(),
    ] {
        assert!(StateContract::from_json(&source, &mut ValueBudget::default()).is_err());
    }
}

#[test]
fn hk_02_06_json_capacity_applies_to_keys_strings_lists_depth_and_shared_copy_budget() {
    for (count, accepted) in [(4096, true), (4097, false)] {
        let source = STATE.replace(UNICODE_SOURCE, &"a".repeat(count));
        assert_eq!(
            StateContract::from_json(&source, &mut ValueBudget::default()).is_ok(),
            accepted
        );
        let name = "x".repeat(count);
        let source = format!(
            "{{\"fields\":{{\"{name}\":{{\"kind\":\"string\"}}}},\"initial\":{{\"{name}\":\"x\"}}}}"
        );
        assert_eq!(
            StateContract::from_json(&source, &mut ValueBudget::default()).is_ok(),
            accepted
        );
    }
    for (count, accepted) in [(MAX_ITEMS, true), (MAX_ITEMS + 1, false)] {
        let source = STATE.replace(
            "\"items\":[]",
            &format!("\"items\":[{}]", vec!["\"x\""; count].join(",")),
        );
        assert_eq!(
            StateContract::from_json(&source, &mut ValueBudget::default()).is_ok(),
            accepted
        );
    }
    let deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    let source = format!("{{\"fields\":{{}},\"initial\":{deep}}}");
    assert!(StateContract::from_json(&source, &mut ValueBudget::default()).is_err());
    let mut budget = ValueBudget::default();
    assert!(budget.key_bytes(MAX_BYTES - 1).is_err());
    assert!(StateContract::from_json(STATE, &mut budget).is_err());
    assert!(StateContract::from_json("{\"fields\":{},\"initial\":{}}", &mut budget).is_err());
}
