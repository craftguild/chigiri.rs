//! Convert validated finite `f64` values to decimals and test exact multiples without epsilon comparisons.

use num_bigint::{BigInt, Sign};

use super::{ValueError, work::Work};

const DIGITS: usize = 1024;

struct Decimal {
    coefficient: BigInt,
    scale: usize,
    digits: usize,
}

impl Decimal {
    fn read(number: f64, work: &mut Work) -> Result<Self, ValueError> {
        if !number.is_finite() {
            return Err(ValueError("finite decimal required"));
        }
        // Reserve for the longest finite decimal representation needed by an `f64`.
        work.charge(325)?;
        let text = if number.fract() == 0.0 {
            format!("{number:.0}")
        } else {
            number.to_string()
        };
        let (mantissa, exponent) =
            text.split_once(['e', 'E'])
                .map_or(Ok((text.as_str(), 0)), |(m, e)| {
                    e.parse::<i32>()
                        .map(|e| (m, e))
                        .map_err(|_| ValueError("decimal exponent"))
                })?;
        if exponent.unsigned_abs() > 324 {
            return Err(ValueError("decimal exponent limit"));
        }
        let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len());
        let mut digits = mantissa.replace('.', "");
        let scale = fraction as i32 - exponent;
        if scale < 0 {
            digits.extend(std::iter::repeat_n('0', (-scale) as usize));
        }
        // Exclude leading zeroes from coefficient digits while conservatively counting significant trailing zeroes.
        let negative = digits.starts_with('-');
        let significant = digits.trim_start_matches('-').trim_start_matches('0');
        let significant = if significant.is_empty() {
            "0"
        } else {
            significant
        };
        let digits = if negative {
            format!("-{significant}")
        } else {
            significant.to_owned()
        };
        let length = significant.len();
        if length > DIGITS || scale > 324 {
            return Err(ValueError("decimal digit limit"));
        }
        work.charge(length * length)?;
        let coefficient =
            BigInt::parse_bytes(digits.as_bytes(), 10).ok_or(ValueError("decimal coefficient"))?;
        Ok(Self {
            coefficient,
            scale: scale.max(0) as usize,
            digits: length,
        })
    }

    fn scaled(self, scale: usize, work: &mut Work) -> Result<(BigInt, usize), ValueError> {
        let exponent = scale - self.scale;
        let digits = self.digits + exponent;
        if digits > DIGITS {
            return Err(ValueError("decimal digit limit"));
        }
        work.charge(digits * (exponent + 1))?;
        Ok((
            self.coefficient * BigInt::from(10u8).pow(exponent as u32),
            digits,
        ))
    }
}

pub(crate) fn multiple(number: f64, divisor: f64, work: &mut Work) -> Result<bool, ValueError> {
    if divisor <= 0.0 || !divisor.is_finite() {
        return Err(ValueError("positive finite multipleOf required"));
    }
    let value = Decimal::read(number, work)?;
    let divisor = Decimal::read(divisor, work)?;
    let scale = value.scale.max(divisor.scale);
    let (value, digits) = value.scaled(scale, work)?;
    let (divisor, divisor_digits) = divisor.scaled(scale, work)?;
    work.charge(digits * divisor_digits)?;
    Ok(value % divisor == BigInt::from(0))
}

/// Validate an exact decimal multiple without binary floating-point ambiguity.
pub(crate) fn intersects(
    low: f64,
    high: f64,
    divisor: f64,
    integer: bool,
    work: &mut Work,
) -> Result<bool, ValueError> {
    if low <= 0.0 && high >= 0.0 {
        return Ok(true);
    }
    let mut step = Decimal::read(divisor, work)?;
    if integer && step.scale > 0 {
        let mut denominator = BigInt::from(10u8).pow(step.scale as u32);
        let mut numerator = step.coefficient.clone();
        while denominator != BigInt::from(0) {
            work.charge((step.digits + step.scale).pow(2))?;
            let remainder = &numerator % &denominator;
            numerator = denominator;
            denominator = remainder;
        }
        step.coefficient /= numerator;
        step.scale = 0;
    }
    let low = Decimal::read(low, work)?;
    let high = Decimal::read(high, work)?;
    let scale = low.scale.max(high.scale).max(step.scale);
    let (low, low_digits) = low.scaled(scale, work)?;
    let (high, high_digits) = high.scaled(scale, work)?;
    let (step, step_digits) = step.scaled(scale, work)?;
    work.charge((low_digits + high_digits) * step_digits)?;
    let ceil = &low / &step
        + BigInt::from(u8::from(
            low.sign() == Sign::Plus && &low % &step != BigInt::from(0),
        ));
    let floor = &high / &step
        - BigInt::from(u8::from(
            high.sign() == Sign::Minus && &high % &step != BigInt::from(0),
        ));
    Ok(ceil <= floor)
}
