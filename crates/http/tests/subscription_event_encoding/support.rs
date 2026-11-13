use chigiri_core::{GraphQlResponse, Object, Value};
use chigiri_http::{
    __encode_subscription_event, EncodedSubscriptionEvent, FinalizedSubscriptionEvent,
    SubscriptionTransport,
};

pub(super) const LARGE_LIMIT: usize = 16 * 1024;

pub(super) fn executed_response() -> GraphQlResponse {
    let mut data = Object::new();
    data.insert("ok", true);
    GraphQlResponse::executed(Value::Object(data))
}

pub(super) fn encode(
    transport: SubscriptionTransport,
    operation_id: Option<&str>,
    response: &GraphQlResponse,
) -> EncodedSubscriptionEvent {
    __encode_subscription_event(transport, operation_id, response, LARGE_LIMIT).unwrap()
}

pub(super) fn delivery_counter(
    event: &FinalizedSubscriptionEvent,
    transport: SubscriptionTransport,
) -> Option<u64> {
    let payload = result_json(event, transport);
    payload["extensions"]["chigiri"]["delivery"]["droppedTotal"].as_u64()
}

pub(super) fn result_json(
    event: &FinalizedSubscriptionEvent,
    transport: SubscriptionTransport,
) -> serde_json::Value {
    let bytes = event.as_bytes();
    match transport {
        SubscriptionTransport::WebSocket => {
            serde_json::from_slice::<serde_json::Value>(bytes).unwrap()["payload"].clone()
        }
        SubscriptionTransport::ServerSentEvents => {
            let json = bytes
                .strip_prefix(b"event: next\ndata: ")
                .unwrap()
                .strip_suffix(b"\n\n")
                .unwrap();
            serde_json::from_slice(json).unwrap()
        }
        SubscriptionTransport::Multipart => {
            let json = bytes
                .strip_prefix(b"--graphql\r\nContent-Type: application/json\r\n\r\n")
                .unwrap()
                .strip_suffix(b"\r\n")
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(json).unwrap()["payload"].clone()
        }
    }
}

pub(super) fn assert_in_order(text: &str, names: &[&str]) {
    let mut position = 0;
    for name in names {
        let found = text[position..].find(name).unwrap();
        position += found + name.len();
    }
}

pub(super) fn assert_finalized_owner_contract() {
    assert_owner::<FinalizedSubscriptionEvent>();
    let _ = <FinalizedSubscriptionEvent as AmbiguousIfClone<_>>::marker;
}

fn assert_owner<T: AsRef<[u8]> + Send + 'static>() {}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}

struct Invalid;

impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
