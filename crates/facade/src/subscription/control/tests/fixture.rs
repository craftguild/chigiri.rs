use chigiri_http::{
    __decode_websocket_client_message, SubscriptionStaticControl, WebSocketClientMessage,
    WebSocketControlPayload,
};
use chigiri_runtime::{GraphQlError, GraphQlResponse};

use super::super::super::delivery::EndpointByteOwner;
use super::super::{HttpHeartbeatStaging, WebSocketControlStaging};
use chigiri_http::SubscriptionTransport;

pub(super) fn bytes(limit: usize) -> EndpointByteOwner {
    EndpointByteOwner::new(limit)
}

pub(super) fn websocket(bytes: &EndpointByteOwner) -> WebSocketControlStaging {
    WebSocketControlStaging::new(bytes.clone())
}

pub(super) fn heartbeat(
    bytes: &EndpointByteOwner,
    transport: SubscriptionTransport,
) -> HttpHeartbeatStaging {
    HttpHeartbeatStaging::new(bytes.clone(), transport).unwrap()
}

pub(super) fn peer_payload(text: &str) -> WebSocketControlPayload {
    let WebSocketClientMessage::Ping(payload) = __decode_websocket_client_message(text).unwrap()
    else {
        panic!("expected peer ping");
    };
    payload
}

pub(super) fn error_response(message: impl Into<String>) -> GraphQlResponse {
    GraphQlResponse::request_error(GraphQlError::new(message.into()).code("CONTROL"))
}

pub(super) const fn heartbeat_control() -> SubscriptionStaticControl {
    SubscriptionStaticControl::Heartbeat
}
