//! Test duplicate keys, numeric precision, resource limits, and container shapes in seed JSON.

use super::Store;
use super::tests::{ir, model};

const SDL: &str = r#"
scalar Json @representation(type:JSON)
type Item { id:ID! value:Json }
type Query { x:Int }
"#;

#[test]
fn rsm_10_seed_json_rejects_duplicate_keys_at_every_level_without_echoing_values() {
    let ir = ir(SDL);
    for json in [
        r#"{"Item":[],"Item":[]}"#,
        r#"{"Item":[{"id":"private-value","id":"x"}]}"#,
        r#"{"Item":[{"id":"x","value":{"a":1,"a":2}}]}"#,
        r#"{"Item":[{"id":"x","value":[{"a":1,"\u0061":2}]}]}"#,
        r#"{"private-value":[]}"#,
        r#"{"Item":[{"id":"private-value"},{"id":"private-value"}]}"#,
    ] {
        let error = Store::from_seed_json(ir.clone(), json).err().unwrap();
        assert!(!format!("{error:?} {error}").contains("private-value"));
    }
    let store = Store::from_seed_json(ir.clone(), r#"{"Item":[{"id":"a","value":{}},{"id":"b","value":[]},{"id":"c","value":[null]},{"id":"d","value":{"$serde_json::private::Number":"text"}}]}"#).unwrap();
    assert_eq!(store.snapshot().size().0, 4);
    let item = model(&ir, "Item");
    let value = ir.models()[item.0].slots["value"];
    let snapshot = store.snapshot();
    assert!(
        snapshot
            .record(item, "a")
            .unwrap()
            .get(value)
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        snapshot
            .record(item, "b")
            .unwrap()
            .get(value)
            .unwrap()
            .as_list()
            .unwrap()
            .is_empty()
    );
    assert!(
        snapshot
            .record(item, "c")
            .unwrap()
            .get(value)
            .unwrap()
            .as_list()
            .unwrap()[0]
            .is_null()
    );
}

#[test]
fn rsm_09_seed_numbers_preserve_full_width_and_reject_rounding_overflow_or_underflow() {
    let ir = ir(SDL);
    for number in [
        "9007199254740992",
        "9007199254740994.0",
        "1606938044258990275541962092341162602522202993782792835301376",
        "1.25",
        "-0.0",
    ] {
        let json = format!("{{\"Item\":[{{\"id\":\"x\",\"value\":{number}}}]}}");
        assert!(Store::from_seed_json(ir.clone(), &json).is_ok());
    }
    for number in [
        "9007199254740993",
        "9007199254740993.0",
        "1e999999",
        "1e-999999",
        "NaN",
    ] {
        let json = format!("{{\"Item\":[{{\"id\":\"x\",\"value\":{number}}}]}}");
        assert!(Store::from_seed_json(ir.clone(), &json).is_err());
    }
}

#[test]
fn rsm_16_seed_json_bytes_depth_arrays_and_strings_have_boundaries_before_expansion() {
    let ir = ir(SDL);
    let within = format!("{{}}{}", " ".repeat(256 * 1024 - 2));
    assert!(Store::from_seed_json(ir.clone(), &within).is_ok());
    assert!(Store::from_seed_json(ir.clone(), &(within + " ")).is_err());
    for (count, valid) in [(1000, true), (1001, false)] {
        let values = vec!["null"; count].join(",");
        let json = format!("{{\"Item\":[{{\"id\":\"x\",\"value\":[{values}]}}]}}");
        assert_eq!(Store::from_seed_json(ir.clone(), &json).is_ok(), valid);
    }
    for (depth, valid) in [(30, true), (31, false)] {
        let json = format!(
            "{{\"Item\":[{{\"id\":\"x\",\"value\":{}null{}}}]}}",
            "[".repeat(depth),
            "]".repeat(depth)
        );
        assert_eq!(Store::from_seed_json(ir.clone(), &json).is_ok(), valid);
    }
    for (length, valid) in [(4096, true), (4097, false)] {
        let json = format!(
            "{{\"Item\":[{{\"id\":\"x\",\"value\":\"{}\"}}]}}",
            "\\u0078".repeat(length)
        );
        assert_eq!(Store::from_seed_json(ir.clone(), &json).is_ok(), valid);
    }
    for (count, valid) in [(1000, true), (1001, false)] {
        let records = (0..count)
            .map(|index| format!("{{\"id\":\"{index}\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            Store::from_seed_json(ir.clone(), &format!("{{\"Item\":[{records}]}}")).is_ok(),
            valid
        );
    }
    let deep = format!(
        "{{\"Item\":[{{\"id\":\"x\",\"value\":{}null{}}}]}}",
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    assert!(Store::from_seed_json(ir, &deep).is_err());
}
