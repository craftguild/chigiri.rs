use crate::{__preflight_websocket_upgrade, RequestHeader};

use super::{VerifiedWebSocketUpgrade, WebSocketHttpVersion, WebSocketUpgradeError};

fn valid<'a>() -> Vec<RequestHeader<'a>> {
    vec![
        RequestHeader::new("host", b"example.test"),
        RequestHeader::new("connection", b"Upgrade"),
        RequestHeader::new("upgrade", b"websocket"),
        RequestHeader::new("sec-websocket-version", b"13"),
        RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
        RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
    ]
}

fn check<'a>(
    name: &str,
    value: &'a [u8],
) -> Result<VerifiedWebSocketUpgrade<'a>, WebSocketUpgradeError> {
    let mut headers = valid();
    headers.iter_mut().find(|h| h.name == name).unwrap().value = value;
    __preflight_websocket_upgrade("GET", WebSocketHttpVersion::Http11, &headers)
}

#[test]
fn accepts_the_rfc_nonce_and_borrows_the_trimmed_key_without_exposing_it_in_debug() {
    let raw = b" \tdGhlIHNhbXBsZSBub25jZQ==\t ";
    let verified = check("sec-websocket-key", raw).unwrap();
    assert_eq!(verified.key(), "dGhlIHNhbXBsZSBub25jZQ==");
    assert_eq!(verified.key().as_ptr(), raw[2..].as_ptr());
    assert_eq!(verified.protocol(), "graphql-transport-ws");
    assert!(!format!("{verified:?}").contains(verified.key()));
}

#[test]
fn combines_all_list_fields_and_matches_only_the_graphql_protocol_case_sensitively() {
    let mut headers = valid();
    headers[1].value = b"keep-alive, ,";
    headers.push(RequestHeader::new("Connection", b" \tUPGRADE "));
    headers[2].value = b"h2c/1.0, WebSocket";
    headers[5].value = b"graphql-ws, GraphQL-Transport-WS";
    headers.push(RequestHeader::new(
        "Sec-WebSocket-Protocol",
        b" , graphql-transport-ws, unrelated",
    ));
    assert!(__preflight_websocket_upgrade("GET", WebSocketHttpVersion::Http11, &headers).is_ok());
    for offered in [
        b"graphql-ws".as_slice(),
        b"GraphQL-Transport-WS",
        b"subscriptions-transport-ws",
        b"prefix-graphql-transport-ws",
        b"graphql-transport-ws-suffix",
        b"",
    ] {
        assert_eq!(
            check("sec-websocket-protocol", offered).unwrap_err(),
            WebSocketUpgradeError::Subprotocol
        );
    }
}

#[test]
fn protocol_offers_are_unique_across_the_whole_case_sensitive_request_list() {
    for offered in [
        b"graphql-transport-ws, graphql-transport-ws".as_slice(),
        b"other, other, graphql-transport-ws",
        b"other, graphql-transport-ws, other",
        b"other, , other, graphql-transport-ws",
        b"other, other",
    ] {
        assert_eq!(
            check("sec-websocket-protocol", offered).unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
    for first in [
        b"graphql-transport-ws".as_slice(),
        b"other, graphql-transport-ws",
    ] {
        let mut headers = valid();
        headers[5].value = first;
        headers.push(RequestHeader::new("Sec-WebSocket-Protocol", first));
        assert_eq!(
            __preflight_websocket_upgrade("GET", WebSocketHttpVersion::Http11, &headers)
                .unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
    assert!(
        check(
            "sec-websocket-protocol",
            b"Other, other, GraphQL-Transport-WS, graphql-transport-ws"
        )
        .is_ok()
    );
}

#[test]
fn rejects_unsupported_http_methods_and_versions_before_other_fields() {
    for method in ["POST", "CONNECT", "get", "HEAD", ""] {
        let error =
            __preflight_websocket_upgrade(method, WebSocketHttpVersion::Http11, &[]).unwrap_err();
        assert_eq!(error, WebSocketUpgradeError::Method);
        let response = error.into_response_parts();
        assert_eq!(response.status, 405);
        assert!(
            response
                .headers
                .iter()
                .any(|h| h.name == "allow" && h.value == "GET")
        );
    }
    assert_eq!(
        __preflight_websocket_upgrade("GET", WebSocketHttpVersion::Unsupported, &valid())
            .unwrap_err(),
        WebSocketUpgradeError::HttpVersion
    );
}

#[test]
fn requires_every_opening_field_and_rejects_duplicate_singletons() {
    for index in 0..valid().len() {
        let mut headers = valid();
        headers.remove(index);
        assert!(
            __preflight_websocket_upgrade("GET", WebSocketHttpVersion::Http11, &headers).is_err()
        );
    }
    for (name, value) in [
        ("Host", b"example.test".as_slice()),
        ("Sec-WebSocket-Version", b"13"),
        ("Sec-WebSocket-Key", b"dGhlIHNhbXBsZSBub25jZQ=="),
    ] {
        let mut headers = valid();
        headers.push(RequestHeader::new(name, value));
        assert_eq!(
            __preflight_websocket_upgrade("GET", WebSocketHttpVersion::Http11, &headers)
                .unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
}

#[test]
fn rejects_malformed_tokens_even_after_a_supported_protocol_was_seen() {
    for value in [
        b"graphql-transport-ws, \"quoted\"".as_slice(),
        b"graphql-transport-ws, two tokens",
        b"graphql-transport-ws,\xff",
        b"graphql-transport-ws\n",
    ] {
        assert_eq!(
            check("sec-websocket-protocol", value).unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
    for (name, value) in [
        ("connection", b"not-upgrade".as_slice()),
        ("connection", b"Upgrade, \"bad\""),
        ("upgrade", b"websocket/13"),
        ("upgrade", b"websocket, bad/"),
        ("upgrade", b"websocket, bad/1/2"),
        ("host", b""),
        ("host", b"host,second"),
        ("host", b"host name"),
    ] {
        assert_eq!(
            check(name, value).unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
}

#[test]
fn requires_single_version_13_and_sends_only_a_safe_supported_version_hint() {
    let rejected = check("sec-websocket-version", b"12").unwrap_err();
    assert_eq!(rejected, WebSocketUpgradeError::WebSocketVersion);
    let response = rejected.into_response_parts();
    assert_eq!(response.status, 426);
    assert!(
        response
            .headers
            .iter()
            .any(|h| h.name == "sec-websocket-version" && h.value == "13")
    );
    for value in [
        b"13,13".as_slice(),
        b"",
        b"\xff",
        b"13\n",
        b"1234",
        b"013",
        b"00",
        b"256",
        b"999",
    ] {
        assert_eq!(
            check("sec-websocket-version", value).unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
    for value in [b"0".as_slice(), b"1", b"12", b"14", b"99", b"100", b"255"] {
        assert_eq!(
            check("sec-websocket-version", value).unwrap_err(),
            WebSocketUpgradeError::WebSocketVersion
        );
    }
}

#[test]
fn checks_exact_nonce_length_alphabet_padding_and_unused_bits() {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for last in alphabet {
        let mut key = *b"AAAAAAAAAAAAAAAAAAAAAA==";
        key[21] = *last;
        assert_eq!(
            check("sec-websocket-key", &key).is_ok(),
            matches!(last, b'A' | b'Q' | b'g' | b'w')
        );
    }
    for value in [
        b"AAAAAAAAAAAAAAAAAAAAAA=".as_slice(),
        b"AAAAAAAAAAAAAAAAAAAAAA===",
        b"AAAAAAAAAAAAAAAAAAAAAAA=",
        b"AAAAAAAAAAAAAAAAAAAAAA  ",
        b"AAAAAAAAAA AAAAAAAAAAA==",
        b"AAAAAAAAAAAAAAAAAAAAA-==",
        b"AAAAAAAAAAAAAAAAAAAAA\xff==",
        b"\nAAAAAAAAAAAAAAAAAAAAAA==",
    ] {
        assert_eq!(
            check("sec-websocket-key", value).unwrap_err(),
            WebSocketUpgradeError::Malformed
        );
    }
}

#[test]
fn rejection_debug_display_and_wire_never_retain_header_values() {
    let error = check("sec-websocket-protocol", b"private-auth-cookie").unwrap_err();
    let diagnostic = format!("{error:?} {error}");
    let response = error.into_response_parts();
    assert_eq!(response.status, 400);
    assert!(!diagnostic.contains("private-auth-cookie"));
    assert!(
        !std::str::from_utf8(&response.body)
            .unwrap()
            .contains("private-auth-cookie")
    );
    assert!(!response.headers.iter().any(|h| h.name == "upgrade"));
}
