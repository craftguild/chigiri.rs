//! Deterministic settings, results, and frame inspection helpers.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::{GraphQlResponse, Object, SubscriptionOverflow, Value};

use super::super::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

pub(super) const EVENT_LIMIT: usize = 1024;

pub(super) fn settings(
    queued_events: usize,
    queue_age_ns: u64,
    send_timeout_ns: u64,
) -> ValidatedSubscriptionSettings {
    let limits = SubscriptionLimits {
        max_queued_events: queued_events,
        max_queued_bytes: 8 * EVENT_LIMIT,
        max_encoded_event_bytes: EVENT_LIMIT,
        max_queue_age: Duration::from_nanos(queue_age_ns),
        send_no_progress_timeout: Duration::from_nanos(send_timeout_ns),
        max_endpoint_encoded_bytes: 16 * EVENT_LIMIT,
        ..SubscriptionLimits::default()
    };
    ValidatedSubscriptionSettings::new(limits, SubscriptionProtocolSettings::default())
}

pub(super) fn outbox(
    settings: &ValidatedSubscriptionSettings,
    owner: EndpointByteOwner,
    overflow: SubscriptionOverflow,
    transport: SubscriptionTransport,
) -> SubscriptionOutbox {
    let operation_id =
        (transport == SubscriptionTransport::WebSocket).then(|| Arc::<str>::from("operation-1"));
    SubscriptionOutbox::new(settings, owner, overflow, transport, operation_id).unwrap()
}

pub(super) fn response(value: &str) -> GraphQlResponse {
    let mut data = Object::new();
    data.insert("value", value);
    GraphQlResponse::executed(Value::Object(data))
}

pub(super) fn enqueue(
    outbox: &mut SubscriptionOutbox,
    value: &str,
    now_ns: u64,
) -> Result<(), SubscriptionOutboxSignal> {
    let reservation = outbox.reserve_before_poll(now_ns)?;
    outbox.encode_and_enqueue(reservation, &response(value), now_ns)
}

pub(super) fn counter(bytes: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(bytes).unwrap();
    let (_, suffix) = text.split_once("\"droppedTotal\":").unwrap();
    if suffix.starts_with("null") {
        return None;
    }
    let digits: String = suffix.chars().take_while(char::is_ascii_digit).collect();
    Some(digits.parse().unwrap())
}

pub(super) fn assert_profile(bytes: &[u8], transport: SubscriptionTransport) {
    match transport {
        SubscriptionTransport::WebSocket => {
            assert!(bytes.starts_with(b"{\"id\":\"operation-1\",\"type\":\"next\""));
            assert!(bytes.ends_with(b"}"));
        }
        SubscriptionTransport::ServerSentEvents => {
            assert!(bytes.starts_with(b"event: next\ndata: "));
            assert!(bytes.ends_with(b"\n\n"));
        }
        SubscriptionTransport::Multipart => {
            assert!(
                bytes.starts_with(
                    b"--graphql\r\nContent-Type: application/json\r\n\r\n{\"payload\":"
                )
            );
            assert!(bytes.ends_with(b"}\r\n"));
        }
    }
}
