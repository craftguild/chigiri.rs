#[path = "websocket_client_message/invalid.rs"]
mod invalid;
#[path = "websocket_client_message/limits.rs"]
mod limits;
#[path = "websocket_client_message/support.rs"]
mod support;

use chigiri_core::Value;
use chigiri_http::{RequestHeader, RequestParts, WebSocketClientMessage, decode_envelope};

use support::{decode, subscribe};

#[test]
fn initialization_control_payloads_accept_omitted_null_and_objects() {
    for text in [
        r#"{"type":"connection_init"}"#,
        r#"{"type":"connection_init","payload":null}"#,
        r#"{"type":"ping"}"#,
        r#"{"type":"ping","payload":null}"#,
        r#"{"type":"pong"}"#,
        r#"{"type":"pong","payload":null}"#,
    ] {
        match decode(text) {
            WebSocketClientMessage::ConnectionInit(payload) => assert!(payload.object().is_none()),
            WebSocketClientMessage::Ping(payload) | WebSocketClientMessage::Pong(payload) => {
                assert!(!payload.has_object());
                assert_eq!(payload.encoded_bytes(), 0);
            }
            _ => panic!("unexpected client message"),
        }
    }

    let message =
        decode(r#"{"type":"connection_init","payload":{"token":"private","attempt":1.0}}"#);
    let WebSocketClientMessage::ConnectionInit(payload) = message else {
        panic!("expected connection init");
    };
    let object = payload.object().unwrap();
    assert_eq!(object.get("token").and_then(Value::as_str), Some("private"));
    assert_eq!(object.get("attempt"), Some(&Value::Int(1)));
    let owner = payload.into_arc().unwrap();
    assert_eq!(owner.get("attempt"), Some(&Value::Int(1)));
}

#[test]
fn pinned_extra_members_do_not_change_client_message_roles() {
    let ping = decode(r#"{"type":"ping","id":17,"extra":{"ignored":true}}"#);
    assert!(matches!(ping, WebSocketClientMessage::Ping(_)));

    let complete =
        decode(r#"{"type":"complete","id":"operation","payload":"ignored","extra":false}"#);
    let WebSocketClientMessage::Complete(operation_id) = complete else {
        panic!("expected complete");
    };
    assert_eq!(operation_id.as_str(), "operation");
    assert_eq!(operation_id.into_string(), "operation");
}

#[test]
fn subscribe_owns_a_normalized_request_envelope() {
    let hash = "a".repeat(64);
    let text = format!(
        concat!(
            "{{\"type\":\"subscribe\",\"id\":\"operation\",\"payload\":{{",
            "\"query\":\"query Named($n: Int) {{ value }}\",",
            "\"operationName\":\"Named\",\"variables\":{{\"n\":1.0}},",
            "\"extensions\":{{\"persistedQuery\":{{\"version\":1,",
            "\"sha256Hash\":\"{}\"}}}},\"extra\":true}}}}"
        ),
        hash
    );
    let request = subscribe(&text);
    assert_eq!(request.operation_id().as_str(), "operation");
    assert_eq!(
        request.envelope().document_text(),
        Some("query Named($n: Int) { value }")
    );
    assert_eq!(request.envelope().operation_name(), Some("Named"));
    assert_eq!(
        request.envelope().variables().get("n"),
        Some(&Value::Int(1))
    );
    let persisted = request.envelope().persisted_reference().unwrap();
    assert_eq!(persisted.id(), hash);
    let document_owner = request.envelope().document_text().unwrap().as_ptr();

    let (operation_id, envelope) = request.into_parts();
    assert_eq!(operation_id.into_string(), "operation");
    assert_eq!(envelope.operation_name(), Some("Named"));
    assert_eq!(envelope.document_text().unwrap().as_ptr(), document_owner);
}

#[test]
fn subscribe_normalization_matches_the_finite_post_envelope() {
    let payload = concat!(
        "{\"query\":\"query Q($n: Int) { value }\",",
        "\"operationName\":null,\"variables\":{\"n\":9007199254740993.0},",
        "\"extensions\":{\"client\":\"fixture\"}}"
    );
    let message = format!("{{\"type\":\"subscribe\",\"id\":\"operation\",\"payload\":{payload}}}");
    let websocket = subscribe(&message);

    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let finite = decode_envelope(RequestParts {
        method: "POST",
        uri_query: None,
        headers: &headers,
        body: payload.as_bytes(),
    })
    .unwrap();
    assert_eq!(websocket.envelope(), &finite.envelope);
}
