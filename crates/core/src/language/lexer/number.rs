use super::{LEX_INVALID_NUMBER, ScanError, ScannedToken, is_name_continue, is_name_start};
use crate::language::TokenKind;

pub(super) fn scan(source: &str, start: usize) -> ScannedToken {
    let bytes = source.as_bytes();
    let mut position = start;
    if bytes.get(position) == Some(&b'-') {
        position += 1;
    }

    if bytes.get(position) == Some(&b'0') {
        position += 1;
        if bytes.get(position).is_some_and(u8::is_ascii_digit) {
            return invalid(
                source,
                start,
                position,
                false,
                name_or_digit_end(bytes, position),
                "Number cannot contain a leading zero",
            );
        }
    } else {
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
    }

    let integer_end = position;
    let mut is_float = false;
    if bytes.get(position) == Some(&b'.') {
        if !bytes.get(position + 1).is_some_and(u8::is_ascii_digit) {
            return invalid(
                source,
                start,
                integer_end,
                false,
                position + 1,
                "Float fraction must contain at least one digit",
            );
        }
        is_float = true;
        position += 2;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
    }

    if matches!(bytes.get(position), Some(b'e' | b'E')) {
        let exponent_start = position;
        let mut exponent_end = position + 1;
        if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
            exponent_end += 1;
        }
        if !bytes.get(exponent_end).is_some_and(u8::is_ascii_digit) {
            return invalid(
                source,
                start,
                exponent_start,
                is_float,
                exponent_end.min(source.len()),
                "Float exponent must contain at least one digit",
            );
        }
        is_float = true;
        position = exponent_end + 1;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
    }

    if bytes.get(position) == Some(&b'.') {
        return invalid(
            source,
            start,
            position,
            is_float,
            position + 1,
            "Number cannot be followed by a decimal point",
        );
    }
    if bytes.get(position).is_some_and(|byte| is_name_start(*byte)) {
        return invalid(
            source,
            start,
            position,
            is_float,
            name_or_digit_end(bytes, position),
            "Number cannot be followed by a name",
        );
    }

    ScannedToken::new(kind(source, start, position, is_float), position, None)
}

fn invalid(
    source: &str,
    start: usize,
    token_end: usize,
    is_float: bool,
    error_end: usize,
    message: &'static str,
) -> ScannedToken {
    ScannedToken::new(
        kind(source, start, token_end, is_float),
        token_end,
        Some(ScanError::new(
            LEX_INVALID_NUMBER,
            message,
            start,
            error_end.min(source.len()),
        )),
    )
}

fn kind(source: &str, start: usize, end: usize, is_float: bool) -> TokenKind {
    let lexeme = source.get(start..end).unwrap_or_default().to_owned();
    if is_float {
        TokenKind::Float(lexeme)
    } else {
        TokenKind::Int(lexeme)
    }
}

fn name_or_digit_end(bytes: &[u8], mut position: usize) -> usize {
    while bytes
        .get(position)
        .is_some_and(|byte| is_name_continue(*byte))
    {
        position += 1;
    }
    position
}
