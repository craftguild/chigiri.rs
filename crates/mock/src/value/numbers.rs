//! Admit only integers that round-trip exactly through Luau's numeric representation.

use chigiri::core::Value;

use super::ValueError;

/// Convert a Luau number without rounding, using the same exactness rules as host encoding.
pub(crate) fn from_luau(value: &luaur::Value) -> Result<Value, ValueError> {
    match value {
        luaur::Value::Integer(value) => {
            // luaur 0.1.8 reports the exactly represented f64 value 2^63 as `i64::MAX`.
            let integer = if *value == i64::MAX {
                1i128 << 63
            } else {
                i128::from(*value)
            };
            let result = Value::Int(integer);
            exact_number(&result)?;
            Ok(result)
        }
        luaur::Value::Number(value) if value.is_finite() => {
            if value.fract() == 0.0 && *value >= i128::MIN as f64 && *value < -(i128::MIN as f64) {
                Ok(Value::Int(*value as i128))
            } else {
                Ok(Value::Float(*value))
            }
        }
        _ => Err(ValueError("finite number required")),
    }
}

/// Convert an exactly representable integer to `f64` without rounding.
pub fn exact_number(value: &Value) -> Result<f64, ValueError> {
    let number = match value {
        Value::Float(value) if value.is_finite() => *value,
        Value::Int(value) => {
            let magnitude = value.unsigned_abs();
            let bits = u128::BITS - magnitude.leading_zeros();
            let discarded = bits.saturating_sub(53);
            if discarded != 0 && magnitude & ((1_u128 << discarded) - 1) != 0 {
                return Err(ValueError("inexact integer"));
            }
            *value as f64
        }
        Value::BigInt(raw) => {
            // The largest finite f64 integer has 309 digits; bound input before parsing and formatting.
            if raw.len() > 310 {
                return Err(ValueError("inexact integer"));
            }
            let number = raw
                .parse::<f64>()
                .map_err(|_| ValueError("invalid integer"))?;
            if !number.is_finite() || format!("{number:.0}") != *raw || raw.parse::<i128>().is_ok()
            {
                return Err(ValueError("inexact integer"));
            }
            number
        }
        _ => return Err(ValueError("finite number required")),
    };
    Ok(number)
}
