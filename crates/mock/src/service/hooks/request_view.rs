//! Build an immutable request snapshot from the same HTTP metadata used by the listener.

use super::copying::Copy;
use crate::limits::HOOK_OBSERVATION_TEXT_BYTES as TEXT_BYTES;
use crate::value::{ValueBudget, ValueError};
use chigiri::{HttpRequestContext, Value};
use std::net::SocketAddr;

pub(super) fn request(
    context: HttpRequestContext<'_>,
    bound: Option<SocketAddr>,
    budget: &mut ValueBudget,
) -> Result<Value, ValueError> {
    let bound = bound.ok_or(ValueError("listener address required"))?;
    let transport = context.transport();
    let mut copy = Copy(budget);
    let method = copy.text(transport.method(), 2)?;
    let host = copy.text(&bound.ip().to_string(), 2)?;
    let port = copy.value(&Value::Int(bound.port().into()), 2)?;
    let uri = transport.uri();
    let url_bytes = uri.path().len() + uri.query().map_or(0, |query| query.len() + 1);
    if url_bytes > TEXT_BYTES {
        return Err(ValueError("request URL limit"));
    }
    copy.0.string_node(url_bytes, 2)?;
    let url = Value::String(match uri.query() {
        Some(query) => format!("{}?{query}", uri.path()),
        None => uri.path().to_owned(),
    });
    let body = copy.text(
        std::str::from_utf8(context.raw_body())
            .map_err(|_| ValueError("UTF-8 request body required"))?,
        2,
    )?;
    copy.0.container(2)?;
    copy.0.items(transport.headers().len())?;
    let mut headers = Vec::new();
    for header in transport.headers() {
        let name = copy.text(header.name, 4)?;
        let value = header_value(header.value, &mut copy)?;
        headers.push(copy.shape("ChigiriHookHeader", vec![name, value], 3)?);
    }
    copy.shape(
        "ChigiriHookRequest",
        vec![method, host, port, url, body, Value::List(headers)],
        1,
    )
}

fn header_value(bytes: &[u8], copy: &mut Copy<'_>) -> Result<Value, ValueError> {
    // Double backslashes in valid UTF-8 so only non-UTF-8 bytes use the unambiguous `\xHH` form.
    let utf8 = std::str::from_utf8(bytes).ok();
    let size = utf8
        .map_or_else(
            || bytes.len().checked_mul(4),
            |text| {
                text.len()
                    .checked_add(text.bytes().filter(|b| *b == b'\\').count())
            },
        )
        .ok_or(ValueError("header size"))?;
    if size > TEXT_BYTES {
        return Err(ValueError("header size"));
    }
    copy.0.string_node(size, 4)?;
    let value = if let Some(text) = utf8 {
        text.replace('\\', "\\\\")
    } else {
        let mut text = String::with_capacity(size);
        use std::fmt::Write;
        for byte in bytes {
            write!(text, "\\x{byte:02X}").expect("write to string");
        }
        text
    };
    Ok(Value::String(value))
}
