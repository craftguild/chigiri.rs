//! The single JSON Content-Type accepted by HTTP subscription ingress.

use crate::RequestHeader;

use super::syntax::{ParameterValue, Pieces, trim};

pub(super) fn accepts_json(headers: &[RequestHeader<'_>]) -> bool {
    let mut values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-type"));
    let Some(header) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(value) = std::str::from_utf8(header.value) else {
        return false;
    };
    let mut pieces = Pieces::new(value, b';');
    if !pieces
        .next()
        .is_some_and(|kind| trim(kind).eq_ignore_ascii_case("application/json"))
    {
        return false;
    }
    let Some(parameter) = pieces.next() else {
        return true;
    };
    let Some((name, value)) = trim(parameter).split_once('=') else {
        return false;
    };
    trim(name).eq_ignore_ascii_case("charset")
        && ParameterValue::parse(trim(value)).is_some_and(|value| value.equals("utf-8", true))
        && pieces.next().is_none()
}
