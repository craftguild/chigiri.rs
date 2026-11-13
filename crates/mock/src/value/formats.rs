//! Validate the eight fixed string formats without normalization or external lookup.

use std::net::{Ipv4Addr, Ipv6Addr};

use base64::Engine as _;
use email_address::{EmailAddress, Options};
use time::{Date, Month, OffsetDateTime, format_description::well_known::Rfc3339};

use super::{MAX_STRING_BYTES, ValueBudget, ValueError};
use crate::ir::Format;

pub(super) fn validate(
    format: Format,
    text: &str,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    if text.len() > MAX_STRING_BYTES {
        return Err(ValueError("format input limit"));
    }
    budget.work.charge(1 + text.len())?;
    let valid = match format {
        Format::Byte => {
            let size = base64::decoded_len_estimate(text.len());
            budget.scratch(size)?;
            let mut decoded = vec![0u8; size];
            base64::engine::general_purpose::STANDARD
                .decode_slice(text, &mut decoded)
                .is_ok()
        }
        Format::Date => date(text),
        Format::DateTime => datetime(text),
        Format::Email => {
            budget.scratch(text.len())?;
            email(text)
        }
        Format::Ipv4 => text.parse::<Ipv4Addr>().is_ok(),
        Format::Ipv6 => text.parse::<Ipv6Addr>().is_ok(),
        Format::Uri => {
            budget.scratch(text.len())?;
            text.is_ascii() && uriparse::URI::try_from(text).is_ok()
        }
        Format::Uuid => {
            text.len() == 36
                && text.bytes().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
                && uuid::Uuid::parse_str(text).is_ok()
        }
    };
    budget.work.charge(0)?;
    valid
        .then_some(())
        .ok_or(ValueError("string format constraint"))
}

fn date(text: &str) -> bool {
    if text.len() != 10
        || !text.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
    {
        return false;
    }
    let year: i32 = text[..4].parse().unwrap();
    let month: u8 = text[5..7].parse().unwrap();
    let day: u8 = text[8..].parse().unwrap();
    year != 0
        && Month::try_from(month)
            .ok()
            .is_some_and(|month| Date::from_calendar_date(year, month, day).is_ok())
}

fn datetime(text: &str) -> bool {
    if !text.is_ascii() || text.len() < 20 || !date(&text[..10]) {
        return false;
    }
    let bytes = text.as_bytes();
    if !matches!(bytes[10], b'T' | b't') || bytes[13] != b':' || bytes[16] != b':' {
        return false;
    }
    let pair = |at: usize, max: u8| {
        bytes[at].is_ascii_digit()
            && bytes[at + 1].is_ascii_digit()
            && (bytes[at] - b'0') * 10 + bytes[at + 1] - b'0' <= max
    };
    if !pair(11, 23) || !pair(14, 59) || !pair(17, 59) {
        return false;
    }
    let mut offset = 19;
    if bytes[offset] == b'.' {
        offset += 1;
        let first = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        if offset == first {
            return false;
        }
    }
    let zone = match bytes.get(offset) {
        Some(b'Z' | b'z') => bytes.len() == offset + 1,
        Some(b'+' | b'-') => {
            bytes.len() == offset + 6
                && bytes[offset + 3] == b':'
                && pair(offset + 1, 23)
                && pair(offset + 4, 59)
        }
        _ => false,
    };
    zone && OffsetDateTime::parse(text, &Rfc3339).is_ok()
}

fn email(text: &str) -> bool {
    if text.len() > 254 || text.chars().any(char::is_control) {
        return false;
    }
    let Some((local, domain)) = text.rsplit_once('@') else {
        return false;
    };
    if local.is_empty() || local.len() > 64 || !domain.is_ascii() || !domain.contains('.') {
        return false;
    }
    if !domain.split('.').all(|label| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes[0].is_ascii_alphanumeric()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
    }) {
        return false;
    }
    let options = Options::default()
        .without_display_text()
        .without_domain_literal();
    EmailAddress::parse_with_options(text, options).is_ok()
}
