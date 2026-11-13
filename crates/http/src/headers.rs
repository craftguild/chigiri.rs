//! Framework-neutral, append-only response headers for response hooks.

use std::error::Error;
use std::fmt;

use crate::ResponseHeader;

/// Invalid header name/value or attempt to override a codec-owned header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponseHeaderError;

impl fmt::Display for ResponseHeaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("response header is invalid or owned by the HTTP codec")
    }
}

impl Error for ResponseHeaderError {}

/// Additional response headers that cannot replace codec-owned headers.
#[derive(Default)]
pub struct ResponseHeaders {
    values: Vec<ResponseHeader>,
}

impl ResponseHeaders {
    /// Creates an empty header collection.
    pub const fn new() -> Self {
        Self { values: Vec::new() }
    }

    /// Validates and appends a header name and value.
    pub fn append(&mut self, name: &str, value: &str) -> Result<(), ResponseHeaderError> {
        let normalized = name.to_ascii_lowercase();
        if !valid_name(name) || forbidden(&normalized) || !valid_value(value) {
            return Err(ResponseHeaderError);
        }
        self.values.push(ResponseHeader {
            name: normalized,
            value: value.to_owned(),
        });
        Ok(())
    }

    /// Internal facade-pipeline entry point for an already validated header.
    #[doc(hidden)]
    pub fn __into_values(self) -> Vec<ResponseHeader> {
        self.values
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
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

fn valid_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || byte >= b' ' && byte != 0x7f)
}

fn forbidden(name: &str) -> bool {
    matches!(
        name,
        "content-type"
            | "content-length"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[cfg(test)]
mod tests {
    use super::ResponseHeaders;

    #[test]
    fn rejects_transport_managed_response_headers() {
        for name in [
            "content-type",
            "content-length",
            "connection",
            "keep-alive",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
        ] {
            assert!(ResponseHeaders::new().append(name, "value").is_err());
        }
    }

    #[test]
    fn rejects_invalid_header_names_and_values() {
        assert!(ResponseHeaders::new().append("x ok", "value").is_err());
        assert!(ResponseHeaders::new().append("x-ok", "a\r\nb").is_err());
    }
}
