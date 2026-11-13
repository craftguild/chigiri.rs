//! Fixed reviewed HTTP/1.1 switching-protocols response.

use axum::body::Body;
use axum::http::header::{
    CONNECTION, HeaderValue, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_PROTOCOL, UPGRADE,
};
use axum::http::{Response, StatusCode};
#[cfg(test)]
use chigiri::__streaming::ClaimedWebSocketUpgrade;
use chigiri::__streaming::HttpStreamingError;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

#[cfg(test)]
pub(super) fn switching(
    seed: &ClaimedWebSocketUpgrade<'_>,
) -> Result<Response<Body>, HttpStreamingError> {
    build(seed.key(), seed.protocol())
}

pub(crate) fn switching_parts(
    key: &str,
    protocol: &'static str,
) -> Result<Response<Body>, HttpStreamingError> {
    build(key, protocol)
}

fn build(key: &str, protocol: &'static str) -> Result<Response<Body>, HttpStreamingError> {
    let accept = derive_accept_key(key.as_bytes());
    let accept = HeaderValue::from_str(&accept).map_err(|_| HttpStreamingError::Invariant)?;
    let protocol = HeaderValue::from_static(protocol);
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("upgrade"));
    response
        .headers_mut()
        .insert(UPGRADE, HeaderValue::from_static("websocket"));
    response.headers_mut().insert(SEC_WEBSOCKET_ACCEPT, accept);
    response
        .headers_mut()
        .insert(SEC_WEBSOCKET_PROTOCOL, protocol);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use axum::body::HttpBody as _;

    use super::*;

    #[test]
    fn switching_response_uses_the_reviewed_accept_key_and_no_length() {
        let response = build("dGhlIHNhbXBsZSBub25jZQ==", "graphql-transport-ws").unwrap();

        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(response.headers()[CONNECTION], "upgrade");
        assert_eq!(response.headers()[UPGRADE], "websocket");
        assert_eq!(
            response.headers()[SEC_WEBSOCKET_ACCEPT],
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert_eq!(
            response.headers()[SEC_WEBSOCKET_PROTOCOL],
            "graphql-transport-ws"
        );
        assert!(
            !response
                .headers()
                .contains_key(axum::http::header::CONTENT_LENGTH)
        );
        assert_eq!(response.body().size_hint().exact(), Some(0));
    }
}
