//! Representative aliases, escaping, errors and application extensions.

use chigiri::core::{GraphQlError, GraphQlResponse, Object, Value};
use chigiri_http::SubscriptionTransport;

pub(super) const PREFIX: &str = "UTF-8: é; quote: \"; slash: \\; controls: \n\t\u{0000}";

pub(super) fn response(padding: usize) -> GraphQlResponse {
    let mut reading = Object::new();
    reading.insert("message", format!("{PREFIX}{}", "a".repeat(padding)));
    reading.insert("number", 7_i32);
    let mut data = Object::new();
    data.insert("selectedAlias", reading);
    let mut error = GraphQlError::new("representative \"error\"\n").code("REPRESENTATIVE_ERROR");
    error.path = vec!["selectedAlias".into(), "optionalField".into()];
    let mut response = GraphQlResponse::executed(Value::Object(data)).with_error(error);
    response
        .extensions
        .insert("application", "escaped\tmetadata");
    response
}

pub(super) fn assert_result(
    bytes: &[u8],
    profile: SubscriptionTransport,
    padding: usize,
    counter: Option<u64>,
    operation_id: Option<&str>,
) {
    let payload = match profile {
        SubscriptionTransport::WebSocket => {
            let envelope: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(envelope["id"], operation_id.unwrap());
            assert_eq!(envelope["type"], "next");
            envelope["payload"].clone()
        }
        SubscriptionTransport::ServerSentEvents => {
            let body = bytes.strip_prefix(b"event: next\ndata: ").unwrap();
            let body = body.strip_suffix(b"\n\n").unwrap();
            serde_json::from_slice(body).unwrap()
        }
        SubscriptionTransport::Multipart => {
            let body = bytes
                .strip_prefix(b"--graphql\r\nContent-Type: application/json\r\n\r\n")
                .unwrap()
                .strip_suffix(b"\r\n")
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(body).unwrap()["payload"].clone()
        }
    };
    let message = payload["data"]["selectedAlias"]["message"]
        .as_str()
        .unwrap();
    assert!(message.starts_with(PREFIX));
    assert_eq!(message.len(), PREFIX.len() + padding);
    assert!(message[PREFIX.len()..].bytes().all(|byte| byte == b'a'));
    assert_eq!(payload["data"]["selectedAlias"]["number"], 7);
    assert_eq!(
        payload["errors"][0]["message"],
        "representative \"error\"\n"
    );
    assert_eq!(
        payload["errors"][0]["extensions"]["code"],
        "REPRESENTATIVE_ERROR"
    );
    assert_eq!(payload["extensions"]["application"], "escaped\tmetadata");
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["droppedTotal"].as_u64(),
        counter
    );
    if counter.is_none() {
        assert!(payload["extensions"]["chigiri"]["delivery"]["droppedTotal"].is_null());
    }
    // The included 16-byte slot is reserved once, independent of its value.
    let text = std::str::from_utf8(bytes).unwrap();
    let marker = "\"droppedTotal\":";
    let start = text.find(marker).unwrap() + marker.len();
    let slot = &text[start..start + 16];
    assert_eq!(slot.trim().parse::<u64>().ok(), counter);
    assert_eq!(text.matches(marker).count(), 1);
}
