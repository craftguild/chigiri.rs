use chigiri_core::GraphQlError;
use chigiri_http::{
    __encode_rejected_websocket_operation_error, __encode_websocket_peer_pong,
    __websocket_peer_pong_len, SubscriptionControlEncodingError, WebSocketClientMessage,
    WebSocketClientMessageError, WebSocketClientMessageErrorKind, WebSocketControlPayload,
    WebSocketObjectPayload, WebSocketOperationId, WebSocketSubscribe,
};

use super::support::{decode, decode_error};

const MIB: usize = 1024 * 1024;
const CONTROL_PAYLOAD_BYTES: usize = 16 * 1024;

#[test]
fn complete_text_limit_is_inclusive_and_checked_before_parse_allocation() {
    let mut exact = r#"{"type":"ping"}"#.to_owned();
    exact.extend(std::iter::repeat_n(' ', MIB - exact.len()));
    assert!(matches!(decode(&exact), WebSocketClientMessage::Ping(_)));

    exact.push(' ');
    assert_eq!(
        decode_error(&exact).kind(),
        WebSocketClientMessageErrorKind::MessageTooLarge
    );
}

#[test]
fn object_payload_limit_charges_the_exact_raw_json_value() {
    let exact_object = format!("{{\"x\":\"{}\"}}", "a".repeat(CONTROL_PAYLOAD_BYTES - 8));
    assert_eq!(exact_object.len(), CONTROL_PAYLOAD_BYTES);
    for kind in ["connection_init", "ping", "pong"] {
        let exact = format!("{{\"type\":\"{kind}\",\"payload\":{exact_object}}}");
        match decode(&exact) {
            WebSocketClientMessage::ConnectionInit(payload) => {
                assert!(payload.object().is_some());
            }
            WebSocketClientMessage::Ping(payload) | WebSocketClientMessage::Pong(payload) => {
                assert_eq!(payload.encoded_bytes(), CONTROL_PAYLOAD_BYTES);
                let framed = __websocket_peer_pong_len(&payload);
                let encoded = __encode_websocket_peer_pong(&payload, framed).unwrap();
                assert_eq!(encoded.len(), framed);
                assert!(__encode_websocket_peer_pong(&payload, framed - 1).is_err());
            }
            _ => panic!("unexpected client message"),
        }

        let oversized_object = format!("{{\"x\":\"{}\"}}", "a".repeat(CONTROL_PAYLOAD_BYTES - 7));
        let oversized = format!("{{\"type\":\"{kind}\",\"payload\":{oversized_object}}}");
        assert_eq!(
            decode_error(&oversized).kind(),
            WebSocketClientMessageErrorKind::PayloadTooLarge
        );
    }
}

#[test]
fn compact_numeric_ping_echoes_without_canonical_expansion() {
    let input = r#"{"type":"ping","payload":{"n":1e70000}}"#;
    let WebSocketClientMessage::Ping(payload) = decode(input) else {
        panic!("expected ping");
    };
    assert!(payload.has_object());
    assert!(payload.encoded_bytes() < CONTROL_PAYLOAD_BYTES);

    let framed = __websocket_peer_pong_len(&payload);
    assert!(framed < 64 * 1024);
    let encoded = __encode_websocket_peer_pong(&payload, usize::MAX).unwrap();
    assert_eq!(encoded.len(), framed);
    assert_eq!(
        encoded.as_bytes(),
        br#"{"type":"pong","payload":{"n":1e70000}}"#
    );
    assert!(matches!(
        __encode_websocket_peer_pong(&payload, framed - 1),
        Err(SubscriptionControlEncodingError::Json(_))
    ));
}

#[test]
fn operation_id_limit_counts_decoded_utf8_bytes() {
    let exact = "x".repeat(256);
    let complete = decode(&format!("{{\"type\":\"complete\",\"id\":\"{exact}\"}}"));
    let WebSocketClientMessage::Complete(operation_id) = complete else {
        panic!("expected complete");
    };
    assert_eq!(operation_id.as_str().len(), 256);

    let escaped = "\\u00e9".repeat(128);
    let complete = decode(&format!("{{\"type\":\"complete\",\"id\":\"{escaped}\"}}"));
    let WebSocketClientMessage::Complete(operation_id) = complete else {
        panic!("expected complete");
    };
    assert_eq!(operation_id.as_str().len(), 256);

    let oversized = "é".repeat(129);
    let error = decode_error(&format!("{{\"type\":\"complete\",\"id\":\"{oversized}\"}}"));
    assert_eq!(
        error.kind(),
        WebSocketClientMessageErrorKind::OperationIdTooLarge
    );
    assert_eq!(error.operation_id().unwrap().as_str(), oversized);
    assert!(!format!("{error:?} {error}").contains(&oversized));
}

#[test]
fn safely_identified_oversized_id_has_one_capped_error_encode_path() {
    let operation_id = "x".repeat(257);
    let input = format!(
        "{{\"type\":\"subscribe\",\"id\":\"{operation_id}\",\"payload\":{{\"query\":\"query {{ value }}\"}}}}"
    );
    let error = decode_error(&input);
    let rejected = error.operation_id().unwrap();
    let response = chigiri_core::GraphQlResponse::request_error(
        GraphQlError::new("operation ID is too long").code("BAD_REQUEST"),
    );
    let encoded =
        __encode_rejected_websocket_operation_error(rejected, &response, 64 * 1024).unwrap();
    let value: serde_json::Value = serde_json::from_slice(encoded.as_bytes()).unwrap();
    assert_eq!(value["id"], operation_id);
    assert_eq!(value["type"], "error");
    assert_eq!(value["payload"][0]["extensions"]["code"], "BAD_REQUEST");

    assert_eq!(
        __encode_rejected_websocket_operation_error(
            rejected,
            &chigiri_core::GraphQlResponse::executed(chigiri_core::Value::Null),
            64 * 1024,
        )
        .unwrap_err(),
        SubscriptionControlEncodingError::MissingErrors
    );
}

#[test]
fn rejected_id_error_encoding_keeps_the_control_hard_cap() {
    let operation_id = "x".repeat(70 * 1024);
    let input = format!("{{\"type\":\"complete\",\"id\":\"{operation_id}\"}}");
    let error = decode_error(&input);
    let response = chigiri_core::GraphQlResponse::request_error(GraphQlError::new("too long"));
    assert!(matches!(
        __encode_rejected_websocket_operation_error(
            error.operation_id().unwrap(),
            &response,
            usize::MAX,
        ),
        Err(SubscriptionControlEncodingError::Json(_))
    ));
}

#[test]
fn deeply_nested_subscribe_values_fail_with_a_redacted_malformed_error() {
    let depth = 140;
    let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
    let input = format!(
        "{{\"type\":\"subscribe\",\"id\":\"operation\",\"payload\":{{\"query\":\"q\",\"variables\":{{\"nested\":{nested}}}}}}}"
    );
    assert_eq!(
        decode_error(&input).kind(),
        WebSocketClientMessageErrorKind::MalformedMessage
    );
}

#[test]
fn accepted_and_rejected_owners_are_non_clone_send_sync_static() {
    assert!(std::mem::size_of::<WebSocketClientMessage>() <= 64);
    assert_owner::<WebSocketClientMessage>();
    assert_owner::<WebSocketControlPayload>();
    assert_owner::<WebSocketObjectPayload>();
    assert_owner::<WebSocketOperationId>();
    assert_owner::<WebSocketSubscribe>();
    assert_owner::<WebSocketClientMessageError>();
    assert_owner::<chigiri_http::RejectedWebSocketOperationId>();

    let _ = <WebSocketClientMessage as AmbiguousIfClone<_>>::marker;
    let _ = <WebSocketControlPayload as AmbiguousIfClone<_>>::marker;
    let _ = <WebSocketObjectPayload as AmbiguousIfClone<_>>::marker;
    let _ = <WebSocketOperationId as AmbiguousIfClone<_>>::marker;
    let _ = <WebSocketSubscribe as AmbiguousIfClone<_>>::marker;
    let _ = <WebSocketClientMessageError as AmbiguousIfClone<_>>::marker;
    let _ = <chigiri_http::RejectedWebSocketOperationId as AmbiguousIfClone<_>>::marker;
}

fn assert_owner<T: Send + Sync + 'static>() {}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}

struct Invalid;

impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
