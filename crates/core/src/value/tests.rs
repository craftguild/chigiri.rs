use super::{Id, Object, Value};

#[test]
fn object_values_preserve_first_insertion_order() {
    let mut object = Object::new();
    assert_eq!(object.insert("second", 2_i32), None);
    assert_eq!(object.insert("first", 1_i32), None);
    assert_eq!(object.insert("second", 20_i32), Some(Value::Int(2)));

    let entries: Vec<_> = object.iter().collect();
    assert_eq!(entries[0], ("second", &Value::Int(20)));
    assert_eq!(entries[1], ("first", &Value::Int(1)));

    assert_eq!(object.remove("second"), Some(Value::Int(20)));
    assert_eq!(object.iter().next(), Some(("first", &Value::Int(1))));
}

#[test]
fn collecting_duplicate_keys_preserves_the_original_position() {
    let object: Object = [("a", 1_i32), ("b", 2_i32), ("a", 3_i32)]
        .into_iter()
        .collect();
    let keys: Vec<_> = object.iter().map(|(key, _)| key).collect();

    assert_eq!(keys, ["a", "b"]);
    assert_eq!(object.get("a"), Some(&Value::Int(3)));
}

#[test]
fn keeps_lookup_index_synchronized_with_insertion_order_after_removal() {
    let mut object: Object = (0_i32..100)
        .map(|index| (format!("key-{index}"), index))
        .collect();

    assert_eq!(object.remove("key-40"), Some(Value::Int(40)));
    assert_eq!(object.get("key-41"), Some(&Value::Int(41)));
    assert_eq!(object.insert("key-41", 410_i32), Some(Value::Int(41)));
    assert_eq!(object.get("key-41"), Some(&Value::Int(410)));
    assert_eq!(object.iter().nth(40).map(|(key, _)| key), Some("key-41"));
}

#[test]
fn builds_and_looks_up_large_objects_through_an_index() {
    let object: Object = (0_u64..10_000)
        .map(|index| (format!("key-{index}"), index))
        .collect();

    assert_eq!(object.len(), 10_000);
    assert_eq!(object.get("key-9999"), Some(&Value::Int(9_999)));
}

#[test]
fn json_integer_conversion_preserves_signed_and_unsigned_values_losslessly() {
    assert_eq!(Value::from(i64::MIN), Value::Int(i128::from(i64::MIN)));
    assert_eq!(Value::from(i64::MAX), Value::Int(i128::from(i64::MAX)));
    assert_eq!(Value::from(u64::MAX), Value::Int(i128::from(u64::MAX)));
}

#[test]
fn converts_ids_between_text_integer_and_value_forms() {
    let text = Id::from("book-01");
    let integer = Id::from(42_i32);

    assert_eq!(text.as_str(), "book-01");
    assert_eq!(integer.as_str(), "42");
    assert_eq!(text.to_string(), "book-01");
    assert_eq!(Value::from(integer), Value::String("42".to_owned()));
    assert_eq!(Id::from(i64::MIN).as_str(), i64::MIN.to_string());
    assert_eq!(Id::from(u64::MAX).as_str(), u64::MAX.to_string());
}
