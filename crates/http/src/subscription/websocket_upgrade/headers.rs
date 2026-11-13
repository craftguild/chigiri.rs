//! Allocation-free field-list and fixed-size nonce validation.

use crate::RequestHeader;

use super::WebSocketUpgradeError;

pub(super) fn trim(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        value = &value[1..];
    }
    while value.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

pub(super) fn singleton<'a>(
    headers: &[RequestHeader<'a>],
    name: &str,
) -> Result<&'a [u8], WebSocketUpgradeError> {
    let mut values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name));
    let value = values.next().ok_or(WebSocketUpgradeError::Malformed)?;
    if values.next().is_some() {
        return Err(WebSocketUpgradeError::Malformed);
    }
    Ok(trim(value.value))
}

pub(super) fn contains(
    headers: &[RequestHeader<'_>],
    name: &str,
    wanted: &[u8],
    insensitive: bool,
    valid: fn(&[u8]) -> bool,
) -> Result<bool, WebSocketUpgradeError> {
    let mut found = false;
    for part in parts(headers, name) {
        if !valid(part) {
            return Err(WebSocketUpgradeError::Malformed);
        }
        found |= if insensitive {
            part.eq_ignore_ascii_case(wanted)
        } else {
            part == wanted
        };
    }
    Ok(found)
}

pub(super) fn protocol(
    headers: &[RequestHeader<'_>],
    wanted: &[u8],
) -> Result<bool, WebSocketUpgradeError> {
    let name = "sec-websocket-protocol";
    let mut found = false;
    for (index, part) in parts(headers, name).enumerate() {
        if !token(part) || parts(headers, name).take(index).any(|prior| prior == part) {
            return Err(WebSocketUpgradeError::Malformed);
        }
        found |= part == wanted;
    }
    // A borrowed prefix scan per offer avoids an input-sized set or index.
    // HTTP parser/header-size and handshake CPU budgets remain app-owned.
    Ok(found)
}

fn parts<'a>(
    headers: &'a [RequestHeader<'_>],
    name: &'a str,
) -> impl Iterator<Item = &'a [u8]> + 'a {
    headers
        .iter()
        .filter(move |header| header.name.eq_ignore_ascii_case(name))
        .flat_map(|header| header.value.split(|byte| *byte == b','))
        .map(trim)
        // HTTP list syntax permits empty elements; they select nothing.
        .filter(|part| !part.is_empty())
}

pub(super) fn token(value: &[u8]) -> bool {
    !value.is_empty()
        && value.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(super) fn upgrade(value: &[u8]) -> bool {
    let mut parts = value.split(|byte| *byte == b'/');
    token(parts.next().unwrap_or_default())
        && parts.next().is_none_or(token)
        && parts.next().is_none()
}

pub(super) fn nonce(value: &[u8]) -> bool {
    // Exactly 16 bytes encode as 22 alphabet characters and two padding bytes.
    // RFC 4648 canonical padding leaves four zero low bits in character 22.
    value.len() == 24
        && &value[22..] == b"=="
        && value[..22]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
        && matches!(value[21], b'A' | b'Q' | b'g' | b'w')
}

pub(super) fn version(value: &[u8]) -> bool {
    if value.is_empty() || value.len() > 3 || (value.len() > 1 && value[0] == b'0') {
        return false;
    }
    value
        .iter()
        .try_fold(0u16, |number, byte| {
            let digit = byte.checked_sub(b'0')?;
            (digit <= 9).then_some(())?;
            number.checked_mul(10)?.checked_add(u16::from(digit))
        })
        .is_some_and(|number| number <= 255)
}
