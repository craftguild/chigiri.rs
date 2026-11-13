use chigiri_http::{
    __decode_websocket_client_message, WebSocketClientMessage, WebSocketClientMessageError,
    WebSocketSubscribe,
};

pub(super) fn decode(text: &str) -> WebSocketClientMessage {
    __decode_websocket_client_message(text).unwrap()
}

pub(super) fn decode_error(text: &str) -> WebSocketClientMessageError {
    __decode_websocket_client_message(text).unwrap_err()
}

pub(super) fn subscribe(text: &str) -> WebSocketSubscribe {
    let WebSocketClientMessage::Subscribe(subscribe) = decode(text) else {
        panic!("expected subscribe message");
    };
    subscribe
}
