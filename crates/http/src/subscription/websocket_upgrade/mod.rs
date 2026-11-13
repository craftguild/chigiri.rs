//! Borrowed opening-handshake checks, not an application admission bypass.

mod headers;
#[cfg(test)]
mod tests;

use std::fmt;

use crate::{RequestHeader, ResponseMediaType, ResponseParts};
use headers::{contains, nonce, singleton, token, trim};

const PROTOCOL: &[u8] = b"graphql-transport-ws";

/// HTTP-version classification supplied by the official server bridge.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketHttpVersion {
    /// The supported HTTP/1.1 opening handshake.
    Http11,
    /// Any other version, including HTTP/2 extended CONNECT.
    Unsupported,
}

/// A verified borrowed nonce for the reviewed library's accept-key calculation.
///
/// This is not a permit to upgrade or start a connection. Application transport
/// admissions, origin/CSRF policy, bounded metadata and connection capacity must
/// still succeed before 101. No body or application callback was polled.
#[doc(hidden)]
pub struct VerifiedWebSocketUpgrade<'a> {
    key: &'a str,
}

impl VerifiedWebSocketUpgrade<'_> {
    /// Returns the validated, OWS-trimmed 16-byte nonce's base64 representation.
    pub const fn key(&self) -> &str {
        self.key
    }

    /// Returns the sole supported subprotocol selected for the response.
    pub const fn protocol(&self) -> &'static str {
        "graphql-transport-ws"
    }
}

impl fmt::Debug for VerifiedWebSocketUpgrade<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedWebSocketUpgrade")
            .field("protocol", &self.protocol())
            .finish_non_exhaustive()
    }
}

/// Closed rejection of an unsupported or malformed opening handshake.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketUpgradeError {
    /// The opening handshake did not use GET.
    Method,
    /// The opening handshake did not use HTTP/1.1.
    HttpVersion,
    /// Required HTTP handshake fields were missing or malformed.
    Malformed,
    /// A well-formed WebSocket version other than 13 was offered.
    WebSocketVersion,
    /// The exact supported GraphQL subprotocol was not offered.
    Subprotocol,
}

impl WebSocketUpgradeError {
    /// Returns a finite, credential-free rejection; this never returns 101.
    pub fn into_response_parts(self) -> ResponseParts {
        let (status, message, code) = match self {
            Self::Method => (405, "WebSocket upgrades require GET", "METHOD_NOT_ALLOWED"),
            Self::WebSocketVersion => (426, "WebSocket version 13 is required", "BAD_REQUEST"),
            _ => (400, "Unsupported WebSocket upgrade request", "BAD_REQUEST"),
        };
        let media = ResponseMediaType::GraphQlResponseJson;
        let response = chigiri_runtime::GraphQlResponse::request_error(
            chigiri_runtime::GraphQlError::new(message).code(code),
        );
        let mut parts = crate::encode_response_with_status(media, status, &response)
            .unwrap_or_else(|_| ResponseParts::serialization_failure(media));
        if self == Self::Method {
            parts.push_header("allow", "GET");
        }
        if self == Self::WebSocketVersion {
            parts.push_header("sec-websocket-version", "13");
        }
        parts
    }
}

impl fmt::Display for WebSocketUpgradeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebSocket opening handshake was rejected")
    }
}

impl std::error::Error for WebSocketUpgradeError {}

pub(crate) fn preflight<'a>(
    method: &str,
    version: WebSocketHttpVersion,
    headers: &[RequestHeader<'a>],
) -> Result<VerifiedWebSocketUpgrade<'a>, WebSocketUpgradeError> {
    use WebSocketUpgradeError as Error;
    if method != "GET" {
        return Err(Error::Method);
    }
    if version != WebSocketHttpVersion::Http11 {
        return Err(Error::HttpVersion);
    }
    let host = singleton(headers, "host")?;
    // Authority routing and allowed-host policy remain with the application.
    if host.is_empty() || !host.iter().all(|b| b.is_ascii_graphic() && *b != b',') {
        return Err(Error::Malformed);
    }
    if !contains(headers, "connection", b"upgrade", true, token)?
        || !contains(headers, "upgrade", b"websocket", true, headers::upgrade)?
    {
        return Err(Error::Malformed);
    }
    let version = singleton(headers, "sec-websocket-version")?;
    if !headers::version(version) {
        return Err(Error::Malformed);
    }
    if version != b"13" {
        return Err(Error::WebSocketVersion);
    }
    let key = singleton(headers, "sec-websocket-key")?;
    if !nonce(key) {
        return Err(Error::Malformed);
    }
    if !headers::protocol(headers, PROTOCOL)? {
        return Err(Error::Subprotocol);
    }
    let key = std::str::from_utf8(trim(key)).map_err(|_| Error::Malformed)?;
    Ok(VerifiedWebSocketUpgrade { key })
}
