//! Converts between core [`Value`] and [`serde_json::Value`] without losing
//! ordering or numeric precision.

use std::cmp::Ordering;

use chigiri_core::{Object, Value};

const MAX_CANONICAL_INTEGER_DIGITS: usize = 1024 * 1024;

/// Failure to represent a core value as `serde_json::Value`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonConversionError {
    /// The value cannot be represented losslessly or within configured limits.
    NumberOutOfRange,
    /// The canonical integer expansion from exponent to resource exceeds the limit.
    NumberExpansionLimit,
    /// NaN or infinity is not present in JSON numbers.
    NonFiniteFloat,
    /// JSON response body has exceeded the set byte limit.
    ResponseSizeLimit,
}

impl std::fmt::Display for JsonConversionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NumberOutOfRange => {
                formatter.write_str("number is outside the supported JSON range")
            }
            Self::NumberExpansionLimit => {
                formatter.write_str("canonical integer expansion limit exceeded")
            }
            Self::NonFiniteFloat => formatter.write_str("float is not finite"),
            Self::ResponseSizeLimit => formatter.write_str("response size limit exceeded"),
        }
    }
}

impl std::error::Error for JsonConversionError {}

/// Converts a Serde JSON value to a core value.
pub fn value_from_json(value: serde_json::Value) -> Result<Value, JsonConversionError> {
    let mut budget = NumberBudget::new(MAX_CANONICAL_INTEGER_DIGITS);
    value_from_json_with_budget(value, &mut budget)
}

fn value_from_json_with_budget(
    value: serde_json::Value,
    budget: &mut NumberBudget,
) -> Result<Value, JsonConversionError> {
    Ok(match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => value_from_number(value, budget)?,
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => Value::List(
            values
                .into_iter()
                .map(|value| value_from_json_with_budget(value, budget))
                .collect::<Result<_, _>>()?,
        ),
        serde_json::Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(name, value)| Ok((name, value_from_json_with_budget(value, budget)?)))
                .collect::<Result<_, JsonConversionError>>()?,
        ),
    })
}

struct NumberBudget {
    remaining_integer_digits: usize,
}

impl NumberBudget {
    const fn new(maximum_integer_digits: usize) -> Self {
        Self {
            remaining_integer_digits: maximum_integer_digits,
        }
    }

    fn consume(&mut self, digits: usize) -> Result<(), JsonConversionError> {
        self.remaining_integer_digits = self
            .remaining_integer_digits
            .checked_sub(digits)
            .ok_or(JsonConversionError::NumberExpansionLimit)?;
        Ok(())
    }
}

fn value_from_number(
    value: serde_json::Number,
    budget: &mut NumberBudget,
) -> Result<Value, JsonConversionError> {
    let raw = value.to_string();
    if let Some(integer) = canonical_integer(&raw, budget.remaining_integer_digits)? {
        budget.consume(integer.trim_start_matches('-').len())?;
        if let Ok(integer) = integer.parse::<i128>() {
            return Ok(Value::Int(integer));
        }
        return Ok(Value::BigInt(integer));
    }

    let float = value
        .as_f64()
        .ok_or(JsonConversionError::NumberOutOfRange)?;
    if float == 0.0 && significand_has_nonzero_digit(&raw) {
        return Err(JsonConversionError::NumberOutOfRange);
    }
    Ok(Value::Float(float))
}

fn canonical_integer(
    raw: &str,
    maximum_digits: usize,
) -> Result<Option<String>, JsonConversionError> {
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw), |unsigned| (true, unsigned));
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, 0), |(mantissa, exponent)| {
            (mantissa, decimal_exponent(exponent))
        });
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = String::with_capacity(whole.len().saturating_add(fraction.len()));
    digits.push_str(whole);
    digits.push_str(fraction);

    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Ok(Some("0".to_owned()));
    }
    digits = significant.to_owned();
    let fractional_digits = i64::try_from(fraction.len()).unwrap_or(i64::MAX);
    let scale = fractional_digits.saturating_sub(exponent);
    match scale.cmp(&0) {
        Ordering::Greater => {
            let Ok(scale) = usize::try_from(scale) else {
                return Ok(None);
            };
            if scale >= digits.len()
                || !digits[digits.len() - scale..]
                    .bytes()
                    .all(|byte| byte == b'0')
            {
                return Ok(None);
            }
            digits.truncate(digits.len() - scale);
        }
        Ordering::Less => {
            let zeros = usize::try_from(scale.saturating_neg())
                .map_err(|_| JsonConversionError::NumberExpansionLimit)?;
            let final_len = digits
                .len()
                .checked_add(zeros)
                .ok_or(JsonConversionError::NumberExpansionLimit)?;
            if final_len > maximum_digits {
                return Err(JsonConversionError::NumberExpansionLimit);
            }
            digits.extend(std::iter::repeat_n('0', zeros));
        }
        Ordering::Equal => {}
    }
    if digits.len() > maximum_digits {
        return Err(JsonConversionError::NumberExpansionLimit);
    }
    if negative {
        digits.insert(0, '-');
    }
    Ok(Some(digits))
}

fn decimal_exponent(raw: &str) -> i64 {
    let (negative, digits) = raw
        .strip_prefix('-')
        .map_or((false, raw.strip_prefix('+').unwrap_or(raw)), |digits| {
            (true, digits)
        });
    let magnitude = digits.bytes().fold(0_i64, |value, byte| {
        value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'))
    });
    if negative {
        magnitude.saturating_neg()
    } else {
        magnitude
    }
}

fn significand_has_nonzero_digit(raw: &str) -> bool {
    raw.split(['e', 'E'])
        .next()
        .unwrap_or(raw)
        .bytes()
        .any(|byte| matches!(byte, b'1'..=b'9'))
}

pub(crate) fn big_int_number(value: &str) -> Result<serde_json::Number, JsonConversionError> {
    if value.parse::<i128>().is_ok() || !canonical_integer_syntax(value) {
        return Err(JsonConversionError::NumberOutOfRange);
    }
    serde_json::from_str(value).map_err(|_| JsonConversionError::NumberOutOfRange)
}

fn canonical_integer_syntax(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        _ => return false,
    }
    index == bytes.len()
}

/// Converts a core value to a Serde JSON value.
pub fn value_to_json(value: &Value) -> Result<serde_json::Value, JsonConversionError> {
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Boolean(value) => serde_json::Value::Bool(*value),
        Value::Int(value) => {
            let number = serde_json::Number::from_i128(*value)
                .ok_or(JsonConversionError::NumberOutOfRange)?;
            serde_json::Value::Number(number)
        }
        Value::BigInt(value) => serde_json::Value::Number(big_int_number(value)?),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or(JsonConversionError::NonFiniteFloat)?,
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::List(values) => {
            serde_json::Value::Array(values.iter().map(value_to_json).collect::<Result<_, _>>()?)
        }
        Value::Object(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(name, value)| Ok((name.to_owned(), value_to_json(value)?)))
                .collect::<Result<_, JsonConversionError>>()?,
        ),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectParseError {
    InvalidJson,
    InvalidObject,
}

pub(crate) fn parse_object(bytes: &[u8]) -> Result<Object, ObjectParseError> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|_| ObjectParseError::InvalidJson)?;
    object_from_json(value)
}

pub(crate) fn parse_object_text(text: &str) -> Result<Object, ObjectParseError> {
    let value = serde_json::from_str::<serde_json::Value>(text)
        .map_err(|_| ObjectParseError::InvalidJson)?;
    object_from_json(value)
}

fn object_from_json(value: serde_json::Value) -> Result<Object, ObjectParseError> {
    let Value::Object(value) =
        value_from_json(value).map_err(|_| ObjectParseError::InvalidObject)?
    else {
        return Err(ObjectParseError::InvalidObject);
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        JsonConversionError, NumberBudget, value_from_json, value_from_json_with_budget,
        value_to_json,
    };
    use chigiri_core::Value;

    #[test]
    fn signed_and_unsigned_boundaries_convert_losslessly_to_core_values() {
        let signed = serde_json::json!(i64::MIN);
        let unsigned = serde_json::json!(u64::MAX);
        assert_eq!(
            value_from_json(signed).unwrap(),
            Value::Int(i128::from(i64::MIN))
        );
        assert_eq!(
            value_from_json(unsigned).unwrap(),
            Value::Int(i128::from(u64::MAX))
        );
    }

    #[test]
    fn normalizes_empty_fractional_parts_into_lossless_integers() {
        for (raw, expected) in [("1.0", 1), ("1.00e2", 100), ("-0.0", 0)] {
            let json = serde_json::from_str(raw).unwrap();
            assert_eq!(
                value_from_json(json).unwrap(),
                Value::Int(expected),
                "{raw}"
            );
        }
        let json = serde_json::from_str("9007199254740993.0").unwrap();
        assert_eq!(
            value_from_json(json).unwrap(),
            Value::Int(9_007_199_254_740_993)
        );
    }

    #[test]
    fn handles_fractional_numbers_and_underflow() {
        let fraction = serde_json::from_str("1.25").unwrap();
        assert_eq!(value_from_json(fraction).unwrap(), Value::Float(1.25));
        for raw in ["1e-400", "-1e-400"] {
            let json = serde_json::from_str(raw).unwrap();
            assert_eq!(
                value_from_json(json),
                Err(JsonConversionError::NumberOutOfRange),
                "{raw}"
            );
        }
    }

    #[test]
    fn expands_bounded_integer_exponents_and_rejects_huge_expansions() {
        for (raw, prefix) in [("1e400", "1"), ("1.5e400", "15")] {
            let json = serde_json::from_str(raw).unwrap();
            let Value::BigInt(integer) = value_from_json(json).unwrap() else {
                panic!("expected the large integer representation");
            };
            assert_eq!(integer.len(), 401, "{raw}");
            assert!(integer.starts_with(prefix), "{raw}");
            assert!(integer[prefix.len()..].bytes().all(|byte| byte == b'0'));
        }

        let json = serde_json::from_str("1e1048576").unwrap();
        assert_eq!(
            value_from_json(json),
            Err(JsonConversionError::NumberExpansionLimit)
        );
    }

    #[test]
    fn shares_the_total_budget_across_nested_values() {
        let within = serde_json::from_str("[1e5,1e5]").unwrap();
        assert!(value_from_json_with_budget(within, &mut NumberBudget::new(12)).is_ok());

        let over = serde_json::from_str("[1e5,1e5]").unwrap();
        assert_eq!(
            value_from_json_with_budget(over, &mut NumberBudget::new(11)),
            Err(JsonConversionError::NumberExpansionLimit)
        );
    }

    #[test]
    fn preserves_i128_and_long_integers_and_rejects_unbounded_floats() {
        for value in [i128::MIN, i128::MAX] {
            let json = value_to_json(&Value::Int(value)).unwrap();
            assert_eq!(value_from_json(json).unwrap(), Value::Int(value));
        }
        let large = "1606938044258990275541962092341162602522202993782792835301376";
        let json = value_to_json(&Value::BigInt(large.to_owned())).unwrap();
        assert_eq!(
            value_from_json(json).unwrap(),
            Value::BigInt(large.to_owned())
        );
        for invalid in ["1", "1.5", "1e2", "01"] {
            assert_eq!(
                value_to_json(&Value::BigInt(invalid.to_owned())),
                Err(JsonConversionError::NumberOutOfRange),
                "{invalid}"
            );
        }
        assert_eq!(
            value_to_json(&Value::Float(f64::INFINITY)),
            Err(JsonConversionError::NonFiniteFloat)
        );
    }
}
