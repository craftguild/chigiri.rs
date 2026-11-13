use chigiri_http::{WebSocketClientMessage, WebSocketClientMessageErrorKind};

use super::support::{decode, decode_error};

#[test]
fn malformed_json_direction_and_message_shapes_share_a_safe_classification() {
    let malformed = [
        "",
        "{",
        "null",
        "[]",
        "{}",
        r#"{"type":null}"#,
        r#"{"type":7}"#,
        r#"{"type":"unknown"}"#,
        r#"{"type":"connection_ack"}"#,
        r#"{"type":"next","id":"operation","payload":{}}"#,
        r#"{"type":"error","id":"operation","payload":[]}"#,
        r#"{"type":"start","id":"operation","payload":{}}"#,
        r#"{"type":"stop","id":"operation"}"#,
        r#"{"type":"connection_terminate"}"#,
        r#"{"type":"ping"} trailing"#,
    ];
    for text in malformed {
        assert_eq!(
            decode_error(text).kind(),
            WebSocketClientMessageErrorKind::MalformedMessage,
            "{text}"
        );
    }
}

#[test]
fn operation_and_object_payload_shapes_are_closed() {
    let malformed = [
        r#"{"type":"subscribe","payload":{"query":"query { value }"}}"#,
        r#"{"type":"subscribe","id":"","payload":{"query":"query { value }"}}"#,
        r#"{"type":"subscribe","id":1,"payload":{"query":"query { value }"}}"#,
        r#"{"type":"subscribe","id":"operation"}"#,
        r#"{"type":"subscribe","id":"operation","payload":null}"#,
        r#"{"type":"subscribe","id":"operation","payload":[]}"#,
        r#"{"type":"subscribe","id":"operation","payload":{}}"#,
        r#"{"type":"subscribe","id":"operation","payload":{"query":null}}"#,
        r#"{"type":"subscribe","id":"operation","payload":{"query":7}}"#,
        r#"{"type":"subscribe","id":"operation","payload":{"query":"q","operationName":7}}"#,
        r#"{"type":"subscribe","id":"operation","payload":{"query":"q","variables":[]}}"#,
        r#"{"type":"subscribe","id":"operation","payload":{"query":"q","extensions":true}}"#,
        r#"{"type":"complete"}"#,
        r#"{"type":"complete","id":""}"#,
        r#"{"type":"complete","id":null}"#,
        r#"{"type":"connection_init","payload":false}"#,
        r#"{"type":"connection_init","payload":[]}"#,
        r#"{"type":"ping","payload":"value"}"#,
        r#"{"type":"pong","payload":1}"#,
    ];
    for text in malformed {
        assert_eq!(
            decode_error(text).kind(),
            WebSocketClientMessageErrorKind::MalformedMessage,
            "{text}"
        );
    }
}

#[test]
fn every_duplicate_protocol_member_is_rejected() {
    let malformed = [
        r#"{"type":"ping","type":"pong"}"#,
        r#"{"type":"complete","id":"a","id":"b"}"#,
        r#"{"type":"ping","payload":null,"payload":{}}"#,
        r#"{"type":"subscribe","id":"a","payload":{"query":"q","query":"other"}}"#,
        r#"{"type":"subscribe","id":"a","payload":{"query":"q","operationName":null,"operationName":"Q"}}"#,
        r#"{"type":"subscribe","id":"a","payload":{"query":"q","variables":{},"variables":null}}"#,
        r#"{"type":"subscribe","id":"a","payload":{"query":"q","extensions":{},"extensions":null}}"#,
    ];
    for text in malformed {
        assert_eq!(
            decode_error(text).kind(),
            WebSocketClientMessageErrorKind::MalformedMessage,
            "{text}"
        );
    }
}

#[test]
fn malformed_subscribe_wins_over_an_oversized_but_parseable_id() {
    let text = format!(
        "{{\"type\":\"subscribe\",\"id\":\"{}\",\"payload\":{{}}}}",
        "x".repeat(257)
    );
    let error = decode_error(&text);
    assert_eq!(
        error.kind(),
        WebSocketClientMessageErrorKind::MalformedMessage
    );
    assert!(error.operation_id().is_none());
}

#[test]
fn accepted_messages_and_failures_redact_raw_values() {
    let private = "private-client-value";
    let init = decode(&format!(
        "{{\"type\":\"connection_init\",\"payload\":{{\"secret\":\"{private}\"}}}}"
    ));
    assert!(!format!("{init:?}").contains(private));

    let subscribe = decode(&format!(
        "{{\"type\":\"subscribe\",\"id\":\"{private}\",\"payload\":{{\"query\":\"query {private} {{ value }}\"}}}}"
    ));
    assert!(!format!("{subscribe:?}").contains(private));

    let error = decode_error(&format!("{{\"type\":\"{private}\"}}"));
    assert!(!format!("{error:?} {error}").contains(private));

    let ping = decode(&format!(
        "{{\"type\":\"ping\",\"payload\":{{\"secret\":\"{private}\"}}}}"
    ));
    assert!(!format!("{ping:?}").contains(private));
}

#[test]
fn complete_ignores_extra_payload_like_the_pinned_validator() {
    let complete = decode(r#"{"type":"complete","id":"operation","payload":[1,2,3]}"#);
    assert!(matches!(complete, WebSocketClientMessage::Complete(_)));
}
