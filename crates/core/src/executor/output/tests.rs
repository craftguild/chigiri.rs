use super::{CustomScalarOutputError, coerce_builtin_scalar, validate_custom_scalar};
use crate::{ExecutionLimits, Object, Value};

#[test]
fn integer_to_float_coercion_requires_exact_representability() {
    assert_eq!(
        coerce_builtin_scalar("Float", Value::Int(1)),
        Ok(Value::Float(1.0))
    );
    assert!(coerce_builtin_scalar("Float", Value::Int(9_007_199_254_740_993)).is_err());

    let exact = "1606938044258990275541962092341162602522202993782792835301376";
    let rounded = "1606938044258990275541962092341162602522202993782792835301377";
    assert!(matches!(
        coerce_builtin_scalar("Float", Value::BigInt(exact.to_owned())),
        Ok(Value::Float(_))
    ));
    assert!(coerce_builtin_scalar("Float", Value::BigInt(rounded.to_owned())).is_err());
}

#[test]
fn builtin_scalars_reject_containers_null_and_nonfinite_values() {
    for value in [
        Value::Null,
        Value::List(Vec::new()),
        Value::Object(Object::new()),
    ] {
        assert!(coerce_builtin_scalar("String", value).is_err());
    }
    assert!(coerce_builtin_scalar("Float", Value::Float(f64::NAN)).is_err());
}

#[test]
fn builtin_scalar_output_is_coerced_to_the_required_type_and_range() {
    assert_eq!(
        coerce_builtin_scalar("Int", Value::Int(i32::MIN.into())),
        Ok(Value::Int(i32::MIN.into()))
    );
    assert!(coerce_builtin_scalar("Int", Value::Int(i128::from(i32::MAX) + 1)).is_err());
    assert_eq!(
        coerce_builtin_scalar("String", Value::String("text".to_owned())),
        Ok(Value::String("text".to_owned()))
    );
    assert!(coerce_builtin_scalar("String", Value::Boolean(true)).is_err());
    assert_eq!(
        coerce_builtin_scalar("Boolean", Value::Boolean(false)),
        Ok(Value::Boolean(false))
    );
    assert_eq!(
        coerce_builtin_scalar("ID", Value::Int(-42)),
        Ok(Value::String("-42".to_owned()))
    );
    assert_eq!(
        coerce_builtin_scalar("ID", Value::String("opaque".to_owned())),
        Ok(Value::String("opaque".to_owned()))
    );
    let large = "170141183460469231731687303715884105728";
    assert_eq!(
        coerce_builtin_scalar("ID", Value::BigInt(large.to_owned())),
        Ok(Value::String(large.to_owned()))
    );
    assert!(coerce_builtin_scalar("Int", Value::BigInt(large.to_owned())).is_err());
}

#[test]
fn custom_scalars_preserve_order_and_count_non_root_nodes() {
    let value = Value::Object(
        [
            ("first", Value::List(vec![Value::Int(1), Value::Null])),
            ("second", Value::Boolean(true)),
        ]
        .into_iter()
        .collect(),
    );
    let validated = validate_custom_scalar(value.clone(), &ExecutionLimits::production()).unwrap();
    let (actual, nodes) = validated.into_parts();

    assert_eq!(actual, value);
    assert_eq!(nodes, 4);
}

#[test]
fn custom_scalar_checks_nested_numeric_and_all_embedded_lists() {
    let invalid = Value::Object(
        [("nested", Value::List(vec![Value::Float(f64::INFINITY)]))]
            .into_iter()
            .collect(),
    );
    assert!(matches!(
        validate_custom_scalar(invalid, &ExecutionLimits::production()),
        Err(CustomScalarOutputError::InvalidNumeric)
    ));

    let mut limits = ExecutionLimits::production();
    limits.max_list_items_per_field = 1;
    let nested = Value::Object(
        [("nested", Value::List(vec![Value::Null, Value::Null]))]
            .into_iter()
            .collect(),
    );
    assert!(matches!(
        validate_custom_scalar(nested, &limits),
        Err(CustomScalarOutputError::ResourceLimit)
    ));
}

#[test]
fn custom_scalar_repeatedly_rejects_deep_invalid_values_without_leaking_state() {
    let mut value = Value::BigInt("01".to_owned());
    for _ in 0..10_001 {
        value = Value::List(vec![value]);
    }
    let mut limits = ExecutionLimits::unbounded();
    limits.max_custom_scalar_output_depth = 10_000;

    assert!(matches!(
        validate_custom_scalar(value, &limits),
        Err(CustomScalarOutputError::ResourceLimit)
    ));
}
