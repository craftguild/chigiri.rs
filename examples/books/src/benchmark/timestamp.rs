//! Convert the canonical UTC expression of the benchmark approval time to Unix seconds.

pub(super) fn parse_canonical_utc_seconds(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 25
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || &bytes[19..] != b"+00:00"
    {
        return None;
    }
    let year = decimal(bytes, 0, 4)? as i64;
    let month = decimal(bytes, 5, 7)? as i64;
    let day = decimal(bytes, 8, 10)? as i64;
    let hour = decimal(bytes, 11, 13)? as i64;
    let minute = decimal(bytes, 14, 16)? as i64;
    let second = decimal(bytes, 17, 19)? as i64;
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    u64::try_from(days.checked_mul(86_400)? + hour * 3_600 + minute * 60 + second).ok()
}

fn decimal(bytes: &[u8], start: usize, end: usize) -> Option<u32> {
    bytes
        .get(start..end)?
        .iter()
        .try_fold(0_u32, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(*byte - b'0'))
        })
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::parse_canonical_utc_seconds;

    #[test]
    fn canonical_utc_uses_second_precision() {
        assert_eq!(
            parse_canonical_utc_seconds("2024-02-29T12:34:56+00:00"),
            Some(1_709_210_096)
        );
        for invalid in [
            "garbage+00:00",
            "2023-02-29T12:34:56+00:00",
            "2024-02-29T12:34:56.1+00:00",
            "2024-02-29T12:34:56Z",
        ] {
            assert_eq!(parse_canonical_utc_seconds(invalid), None);
        }
    }
}
