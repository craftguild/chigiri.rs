//! Bounded exact conversion of GraphQL numeric lexemes and canonicalization of IDs.

use crate::Value;

pub(crate) fn exact_float_from_integer(value: i128) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    let significant_bits = u128::BITS - magnitude.leading_zeros();
    let discarded_bits = significant_bits.saturating_sub(53);
    let exactly_representable =
        discarded_bits == 0 || magnitude & ((1_u128 << discarded_bits) - 1) == 0;
    exactly_representable.then_some(value as f64)
}

pub(crate) fn exact_float_from_integer_lexeme(raw: &str) -> Option<f64> {
    let value = finite_float_from_lexeme(raw)?;
    (format!("{value:.0}") == raw).then_some(value)
}

pub(crate) fn finite_float_from_lexeme(raw: &str) -> Option<f64> {
    let value = raw.parse::<f64>().ok().filter(|value| value.is_finite())?;
    let significand = raw.split(['e', 'E']).next().unwrap_or(raw);
    let nonzero = significand.bytes().any(|byte| matches!(byte, b'1'..=b'9'));
    (!(value == 0.0 && nonzero)).then_some(value)
}

/// Converts an integral decimal value exactly without a floating-point round trip.
/// Fractional decimals retain the same finite `f64` semantics as JSON conversion.
pub(crate) fn integer_float_matches_lexeme(raw: &str, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    if value == 0.0 || value.fract() != 0.0 {
        return true;
    }
    let unsigned = raw.strip_prefix('-').unwrap_or(raw);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, 0), |(mantissa, exponent)| {
            (mantissa, decimal_exponent(exponent))
        });
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = whole.bytes().chain(fraction.bytes());
    let leading = digits.clone().take_while(|byte| *byte == b'0').count();
    let significant = whole.len() + fraction.len() - leading;
    let scale = i64::try_from(fraction.len())
        .unwrap_or(i64::MAX)
        .saturating_sub(exponent);
    let trimming = usize::try_from(scale.max(0)).unwrap_or(usize::MAX);
    let Some(retained) = significant
        .checked_sub(trimming)
        .filter(|count| *count != 0)
    else {
        return true;
    };
    if digits
        .clone()
        .skip(leading + retained)
        .any(|byte| byte != b'0')
    {
        return true;
    }
    let zeros = usize::try_from(scale.saturating_neg().max(0)).unwrap_or(usize::MAX);
    // A finite f64 integer has at most 309 decimal digits. Compare through bounded iterators
    // instead of constructing an exponentially large intermediate string.
    let rounded = format!("{:.0}", value.abs());
    if retained.checked_add(zeros) != Some(rounded.len()) {
        return false;
    }
    digits
        .skip(leading)
        .take(retained)
        .chain(std::iter::repeat_n(b'0', zeros))
        .eq(rounded.bytes())
}

fn decimal_exponent(raw: &str) -> i64 {
    let (negative, digits) = raw
        .strip_prefix('-')
        .map_or((false, raw.strip_prefix('+').unwrap_or(raw)), |digits| {
            (true, digits)
        });
    let value = digits.bytes().fold(0_i64, |value, byte| {
        value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'))
    });
    if negative {
        value.saturating_neg()
    } else {
        value
    }
}

pub(crate) fn canonical_id_from_integer_lexeme(raw: &str) -> Option<String> {
    if !crate::language::numeric::valid_int(raw) {
        return None;
    }
    Some(if raw == "-0" { "0" } else { raw }.to_owned())
}

pub(crate) fn value_from_integer_lexeme(raw: &str) -> Option<Value> {
    let canonical = canonical_id_from_integer_lexeme(raw)?;
    Some(match canonical.parse::<i128>() {
        Ok(value) => Value::Int(value),
        Err(_) => Value::BigInt(canonical),
    })
}

pub(crate) fn valid_big_int_lexeme(raw: &str) -> bool {
    crate::language::numeric::valid_int(raw) && raw.parse::<i128>().is_err()
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_id_from_integer_lexeme, exact_float_from_integer,
        exact_float_from_integer_lexeme, finite_float_from_lexeme, valid_big_int_lexeme,
        value_from_integer_lexeme,
    };

    #[test]
    fn handles_the_two_f64_integer_boundaries() {
        assert_eq!(
            exact_float_from_integer(9_007_199_254_740_992),
            Some(9_007_199_254_740_992.0)
        );
        assert_eq!(exact_float_from_integer(9_007_199_254_740_993), None);
        assert!(exact_float_from_integer(i128::MIN).is_some());
        assert_eq!(exact_float_from_integer(i128::MAX), None);
    }

    #[test]
    fn integer_lexeme_handles_exact_powers_of_two_beyond_i128() {
        const TWO_TO_200: &str = "1606938044258990275541962092341162602522202993782792835301376";
        const TWO_TO_200_PLUS_ONE: &str =
            "1606938044258990275541962092341162602522202993782792835301377";
        const TWO_TO_1024: &str = concat!(
            "17976931348623159077293051907890247336179769789423065727343008115",
            "77326758055009631327084773224075360211201138798713933576587897688",
            "14416622492847430639474124377767893424865485276302219601246094119",
            "45308295208500576883815068234246288147391311054082723716335051068",
            "4586298239947245938479716304835356329624224137216"
        );

        assert!(exact_float_from_integer_lexeme(TWO_TO_200).is_some());
        assert_eq!(exact_float_from_integer_lexeme(TWO_TO_200_PLUS_ONE), None);
        assert_eq!(exact_float_from_integer_lexeme(TWO_TO_1024), None);
    }

    #[test]
    fn float_lexeme_accepts_zero_and_rejects_underflow() {
        for raw in ["0", "0.0", "-0.0", "0e999"] {
            assert!(finite_float_from_lexeme(raw).is_some(), "{raw}");
        }
        for raw in ["1e-400", "-1e-400", "1e400"] {
            assert_eq!(finite_float_from_lexeme(raw), None, "{raw}");
        }
    }

    #[test]
    fn id_integer_lexeme_preserves_arbitrary_positive_values_and_normalizes_negative_zero() {
        let large = "170141183460469231731687303715884105728";
        assert_eq!(
            canonical_id_from_integer_lexeme(large).as_deref(),
            Some(large)
        );
        assert_eq!(canonical_id_from_integer_lexeme("-0").as_deref(), Some("0"));
        assert_eq!(canonical_id_from_integer_lexeme("01"), None);
        assert_eq!(
            value_from_integer_lexeme(large),
            Some(crate::Value::BigInt(large.to_owned()))
        );
        assert!(valid_big_int_lexeme(large));
        for invalid in ["1", "1.5", "1e2", "01"] {
            assert!(!valid_big_int_lexeme(invalid), "{invalid}");
        }
    }
}
