#[path = "subscription_control_encoding/support.rs"]
mod support;
#[path = "subscription_control_encoding/validation.rs"]
mod validation;

use chigiri_core::{GraphQlError, GraphQlResponse, Location, Object, Value};
use chigiri_http::{
    __encode_subscription_control, __encode_subscription_event, __subscription_static_control_len,
    JsonConversionError, SubscriptionControl, SubscriptionControlEncodingError,
    SubscriptionStaticControl, SubscriptionTransport,
};

use support::{LARGE_LIMIT, encode, terminal_response};

#[test]
fn static_entries_have_exact_bytes_lengths_and_bounds() {
    let cases = [
        (
            SubscriptionTransport::WebSocket,
            SubscriptionStaticControl::ConnectionAck,
            SubscriptionControl::ConnectionAck,
            b"{\"type\":\"connection_ack\"}".as_slice(),
        ),
        (
            SubscriptionTransport::WebSocket,
            SubscriptionStaticControl::Heartbeat,
            SubscriptionControl::Heartbeat,
            b"{\"type\":\"ping\"}".as_slice(),
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionStaticControl::Heartbeat,
            SubscriptionControl::Heartbeat,
            b":\n\n".as_slice(),
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionStaticControl::Complete,
            SubscriptionControl::Complete {
                websocket_operation_id: None,
            },
            b"event: complete\ndata: \n\n".as_slice(),
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionStaticControl::Heartbeat,
            SubscriptionControl::Heartbeat,
            b"--graphql\r\nContent-Type: application/json\r\n\r\n{}\r\n".as_slice(),
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionStaticControl::Complete,
            SubscriptionControl::Complete {
                websocket_operation_id: None,
            },
            b"--graphql--".as_slice(),
        ),
    ];

    for (transport, static_control, control, expected) in cases {
        let length = __subscription_static_control_len(transport, static_control).unwrap();
        assert_eq!(length, expected.len());
        let encoded = encode(transport, control, length);
        assert_eq!(encoded.as_bytes(), expected);
        assert_eq!(encoded.len(), length);
        assert_eq!(
            __encode_subscription_control(transport, control_for(static_control), length - 1)
                .unwrap_err(),
            SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit)
        );
    }

    for (transport, control) in [
        (
            SubscriptionTransport::WebSocket,
            SubscriptionStaticControl::Complete,
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionStaticControl::ConnectionAck,
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionStaticControl::ConnectionAck,
        ),
    ] {
        assert_eq!(
            __subscription_static_control_len(transport, control).unwrap_err(),
            SubscriptionControlEncodingError::InvalidTransportArguments
        );
    }
}

#[test]
fn websocket_dynamic_entries_follow_the_pinned_protocol() {
    let complete = encode(
        SubscriptionTransport::WebSocket,
        SubscriptionControl::Complete {
            websocket_operation_id: Some("op-\"1"),
        },
        LARGE_LIMIT,
    );
    assert_eq!(complete.as_bytes(), br#"{"id":"op-\"1","type":"complete"}"#);

    let pong = encode(
        SubscriptionTransport::WebSocket,
        SubscriptionControl::Pong { payload: None },
        LARGE_LIMIT,
    );
    assert_eq!(pong.as_bytes(), br#"{"type":"pong"}"#);

    let mut payload = Object::new();
    payload.insert("nonce", "safe");
    payload.insert("attempt", 2_i32);
    let pong = encode(
        SubscriptionTransport::WebSocket,
        SubscriptionControl::Pong {
            payload: Some(&payload),
        },
        LARGE_LIMIT,
    );
    assert_eq!(
        pong.as_bytes(),
        br#"{"type":"pong","payload":{"nonce":"safe","attempt":2}}"#
    );

    let response = terminal_response();
    let error = encode(
        SubscriptionTransport::WebSocket,
        SubscriptionControl::Error {
            websocket_operation_id: Some("operation"),
            response: &response,
            dropped_total: None,
        },
        LARGE_LIMIT,
    );
    let value: serde_json::Value = serde_json::from_slice(error.as_bytes()).unwrap();
    assert_eq!(
        error.as_bytes(),
        br#"{"id":"operation","type":"error","payload":[{"message":"terminal failure","locations":[{"line":3,"column":8}],"path":["viewer",1],"extensions":{"code":"TERMINAL","safe":true}}]}"#
    );
    assert_eq!(value["id"], "operation");
    assert_eq!(value["type"], "error");
    assert_eq!(value["payload"][0]["locations"][0]["line"], 3);
    assert_eq!(
        value["payload"][0]["path"],
        serde_json::json!(["viewer", 1])
    );
    assert_eq!(value["payload"][0]["extensions"]["code"], "TERMINAL");
}

#[test]
fn sse_error_is_one_next_entry_followed_by_a_separate_complete_entry() {
    let response = terminal_response();
    let error = encode(
        SubscriptionTransport::ServerSentEvents,
        SubscriptionControl::Error {
            websocket_operation_id: None,
            response: &response,
            dropped_total: Some(7),
        },
        LARGE_LIMIT,
    );
    let text = std::str::from_utf8(error.as_bytes()).unwrap();
    assert!(text.starts_with("event: next\ndata: "));
    assert!(text.ends_with("\n\n"));
    assert!(!text.contains("event: complete"));
    let json = text
        .strip_prefix("event: next\ndata: ")
        .unwrap()
        .strip_suffix("\n\n")
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(
        text,
        concat!(
            "event: next\ndata: {\"errors\":[{\"message\":\"terminal failure\",",
            "\"locations\":[{\"line\":3,\"column\":8}],\"path\":[\"viewer\",1],",
            "\"extensions\":{\"code\":\"TERMINAL\",\"safe\":true}}],",
            "\"extensions\":{\"chigiri\":{\"delivery\":{\"droppedTotal\":",
            "7               }}}}\n\n"
        )
    );
    assert_eq!(value["errors"][0]["path"], serde_json::json!(["viewer", 1]));
    assert_eq!(
        value["extensions"]["chigiri"]["delivery"]["droppedTotal"],
        7
    );

    let complete = encode(
        SubscriptionTransport::ServerSentEvents,
        SubscriptionControl::Complete {
            websocket_operation_id: None,
        },
        LARGE_LIMIT,
    );
    assert_eq!(complete.as_bytes(), b"event: complete\ndata: \n\n");
}

#[test]
fn multipart_terminal_errors_project_top_level_positions_and_close_separately() {
    let response = terminal_response();
    let error = encode(
        SubscriptionTransport::Multipart,
        SubscriptionControl::Error {
            websocket_operation_id: None,
            response: &response,
            dropped_total: None,
        },
        LARGE_LIMIT,
    );
    let json = error
        .as_bytes()
        .strip_prefix(b"--graphql\r\nContent-Type: application/json\r\n\r\n")
        .unwrap()
        .strip_suffix(b"\r\n")
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(json).unwrap();
    assert_eq!(
        error.as_bytes(),
        concat!(
            "--graphql\r\nContent-Type: application/json\r\n\r\n",
            "{\"payload\":null,\"errors\":[{\"message\":\"terminal failure\",",
            "\"extensions\":{\"code\":\"TERMINAL\",\"safe\":true}}]}\r\n"
        )
        .as_bytes()
    );
    assert_eq!(value["payload"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["message"], "terminal failure");
    assert_eq!(value["errors"][0]["extensions"]["code"], "TERMINAL");
    assert!(value["errors"][0].get("locations").is_none());
    assert!(value["errors"][0].get("path").is_none());
    assert!(!error.as_bytes().ends_with(b"--graphql--"));

    let complete = encode(
        SubscriptionTransport::Multipart,
        SubscriptionControl::Complete {
            websocket_operation_id: None,
        },
        LARGE_LIMIT,
    );
    assert_eq!(complete.as_bytes(), b"--graphql--");
}

#[test]
fn ordinary_event_errors_keep_locations_and_paths() {
    let response = GraphQlResponse::executed(Value::Null).with_error(terminal_error());
    let event = __encode_subscription_event(
        SubscriptionTransport::Multipart,
        None,
        &response,
        LARGE_LIMIT,
    )
    .unwrap()
    .finalize(None)
    .unwrap();
    let json = event
        .as_bytes()
        .strip_prefix(b"--graphql\r\nContent-Type: application/json\r\n\r\n")
        .unwrap()
        .strip_suffix(b"\r\n")
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(json).unwrap();
    assert_eq!(value["payload"]["errors"][0]["locations"][0]["line"], 3);
    assert_eq!(
        value["payload"]["errors"][0]["path"],
        serde_json::json!(["viewer", 1])
    );
}

fn control_for(control: SubscriptionStaticControl) -> SubscriptionControl<'static> {
    match control {
        SubscriptionStaticControl::ConnectionAck => SubscriptionControl::ConnectionAck,
        SubscriptionStaticControl::Heartbeat => SubscriptionControl::Heartbeat,
        SubscriptionStaticControl::Complete => SubscriptionControl::Complete {
            websocket_operation_id: None,
        },
    }
}

fn terminal_error() -> GraphQlError {
    let mut error = GraphQlError::new("terminal failure").code("TERMINAL");
    error.locations.push(Location::new(3, 8));
    error.path.push("viewer".into());
    error.path.push(1_usize.into());
    error.extensions.insert("safe", true);
    error
}
