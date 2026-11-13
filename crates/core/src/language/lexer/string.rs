use super::{
    LEX_INVALID_ESCAPE, LEX_INVALID_STRING_CHARACTER, LEX_INVALID_UNICODE_ESCAPE,
    LEX_UNTERMINATED_STRING, ScanError, ScannedToken,
};
use crate::language::TokenKind;

pub(super) fn scan(source: &str, start: usize) -> ScannedToken {
    let mut position = start + 1;
    let mut value = String::new();
    let mut first_error = None;

    while position < source.len() {
        match source.as_bytes()[position] {
            b'"' => {
                position += 1;
                return ScannedToken::new(TokenKind::String(value), position, first_error);
            }
            b'\\' => {
                let escaped = scan_escape(source, position);
                position = escaped.end;
                if let Some(character) = escaped.character {
                    value.push(character);
                }
                remember_first(&mut first_error, escaped.error);
            }
            b'\r' => {
                remember_first(
                    &mut first_error,
                    Some(ScanError::new(
                        LEX_INVALID_STRING_CHARACTER,
                        "Line terminator is not allowed in a string",
                        position,
                        position + 1,
                    )),
                );
                position += 1;
                if source.as_bytes().get(position) == Some(&b'\n') {
                    position += 1;
                }
            }
            b'\n' => {
                remember_first(
                    &mut first_error,
                    Some(ScanError::new(
                        LEX_INVALID_STRING_CHARACTER,
                        "Line terminator is not allowed in a string",
                        position,
                        position + 1,
                    )),
                );
                position += 1;
            }
            _ => {
                let Some(character) = source[position..].chars().next() else {
                    break;
                };
                value.push(character);
                position += character.len_utf8();
            }
        }
    }

    ScannedToken::new(
        TokenKind::String(value),
        source.len(),
        Some(ScanError::new(
            LEX_UNTERMINATED_STRING,
            "Unterminated string",
            start,
            source.len(),
        )),
    )
}

fn remember_first(target: &mut Option<ScanError>, error: Option<ScanError>) {
    if target.is_none() {
        *target = error;
    }
}

struct Escaped {
    character: Option<char>,
    end: usize,
    error: Option<ScanError>,
}

fn scan_escape(source: &str, start: usize) -> Escaped {
    let escaped_position = start + 1;
    let Some(&escaped) = source.as_bytes().get(escaped_position) else {
        return invalid_escape(source, start, source.len());
    };
    let simple = match escaped {
        b'"' => Some('"'),
        b'\\' => Some('\\'),
        b'/' => Some('/'),
        b'b' => Some('\u{0008}'),
        b'f' => Some('\u{000C}'),
        b'n' => Some('\n'),
        b'r' => Some('\r'),
        b't' => Some('\t'),
        b'u' => return scan_unicode(source, start, escaped_position + 1),
        _ => None,
    };
    if let Some(character) = simple {
        Escaped {
            character: Some(character),
            end: escaped_position + 1,
            error: None,
        }
    } else {
        let width = source[escaped_position..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        invalid_escape(source, start, escaped_position + width)
    }
}

fn invalid_escape(source: &str, start: usize, end: usize) -> Escaped {
    Escaped {
        character: None,
        end: end.max(start + 1).min(source.len()),
        error: Some(ScanError::new(
            LEX_INVALID_ESCAPE,
            "Invalid escape sequence",
            start,
            end.max(start + 1).min(source.len()),
        )),
    }
}

fn scan_unicode(source: &str, start: usize, digits_start: usize) -> Escaped {
    if source.as_bytes().get(digits_start) == Some(&b'{') {
        return scan_variable_unicode(source, start, digits_start + 1);
    }
    let Some((leading, leading_end)) = four_hex_digits(source, digits_start) else {
        return invalid_unicode(source, start, fixed_recovery_end(source, digits_start));
    };

    if (0xD800..=0xDBFF).contains(&leading) {
        let trailing_start = leading_end + 2;
        if !source
            .get(leading_end..)
            .is_some_and(|rest| rest.starts_with("\\u"))
        {
            return invalid_unicode(source, start, leading_end);
        }
        let Some((trailing, trailing_end)) = four_hex_digits(source, trailing_start) else {
            return invalid_unicode(source, start, leading_end);
        };
        if !(0xDC00..=0xDFFF).contains(&trailing) {
            return invalid_unicode(source, start, leading_end);
        }
        let scalar =
            ((u32::from(leading) - 0xD800) * 0x400) + (u32::from(trailing) - 0xDC00) + 0x10000;
        return unicode_character(source, start, trailing_end, scalar);
    }
    if (0xDC00..=0xDFFF).contains(&leading) {
        return invalid_unicode(source, start, leading_end);
    }
    unicode_character(source, start, leading_end, u32::from(leading))
}

fn scan_variable_unicode(source: &str, start: usize, digits_start: usize) -> Escaped {
    let mut position = digits_start;
    let mut value = 0_u32;
    let mut digit_count = 0_usize;
    let mut overflow = false;
    while let Some(&byte) = source.as_bytes().get(position) {
        let Some(digit) = hex_value(byte) else {
            break;
        };
        digit_count += 1;
        value = value
            .checked_mul(16)
            .and_then(|current| current.checked_add(u32::from(digit)))
            .unwrap_or_else(|| {
                overflow = true;
                0
            });
        position += 1;
    }
    if source.as_bytes().get(position) != Some(&b'}') {
        return invalid_unicode(source, start, variable_recovery_end(source, position));
    }
    position += 1;
    if digit_count == 0 || overflow {
        return invalid_unicode(source, start, position);
    }
    unicode_character(source, start, position, value)
}

fn unicode_character(source: &str, start: usize, end: usize, scalar: u32) -> Escaped {
    let Some(character) = char::from_u32(scalar) else {
        return invalid_unicode(source, start, end);
    };
    Escaped {
        character: Some(character),
        end,
        error: None,
    }
}

fn invalid_unicode(source: &str, start: usize, end: usize) -> Escaped {
    let end = end.max(start + 2).min(source.len());
    Escaped {
        character: None,
        end,
        error: Some(ScanError::new(
            LEX_INVALID_UNICODE_ESCAPE,
            "Invalid Unicode escape sequence",
            start,
            end,
        )),
    }
}

fn four_hex_digits(source: &str, start: usize) -> Option<(u16, usize)> {
    let bytes = source.as_bytes().get(start..start.checked_add(4)?)?;
    let mut value = 0_u16;
    for byte in bytes {
        value = value.checked_mul(16)? + u16::from(hex_value(*byte)?);
    }
    Some((value, start + 4))
}

fn fixed_recovery_end(source: &str, start: usize) -> usize {
    let mut position = start;
    for _ in 0..4 {
        let Some(character) = source[position..].chars().next() else {
            break;
        };
        if matches!(character, '"' | '\\' | '\r' | '\n') {
            break;
        }
        position += character.len_utf8();
    }
    position
}

fn variable_recovery_end(source: &str, start: usize) -> usize {
    let mut position = start;
    while let Some(character) = source[position..].chars().next() {
        if matches!(character, '"' | '\\' | '\r' | '\n') {
            break;
        }
        position += character.len_utf8();
        if character == '}' {
            break;
        }
    }
    position
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
