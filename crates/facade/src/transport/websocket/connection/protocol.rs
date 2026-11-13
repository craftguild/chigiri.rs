//! Single authoritative post-ack protocol classifier.

use std::sync::Arc;

use chigiri_http::WebSocketClientMessage;

use super::authorization::{AuthorizationErrorObserver, AuthorizationFrameworkFailure};
use super::close::WebSocketConnectionClose;
use super::input::InitializedWebSocketInput;
use crate::subscription::control::WebSocketControlStaging;

pub(super) fn receive_text(
    controls: &mut WebSocketControlStaging,
    observer: &Arc<dyn AuthorizationErrorObserver>,
    terminal: &mut Option<WebSocketConnectionClose>,
    text: &str,
) -> Result<InitializedWebSocketInput, WebSocketConnectionClose> {
    if let Some(close) = *terminal {
        return Err(close);
    }
    let message = match chigiri_http::__decode_websocket_client_message(text) {
        Ok(message) => message,
        Err(error) => {
            if let Some(operation_id) = error.into_operation_id() {
                return Ok(InitializedWebSocketInput::RejectedOperationId(operation_id));
            }
            return Err(request_close(
                terminal,
                WebSocketConnectionClose::BAD_REQUEST,
            ));
        }
    };
    match message {
        WebSocketClientMessage::ConnectionInit(_) => Err(request_close(
            terminal,
            WebSocketConnectionClose::TOO_MANY_INIT,
        )),
        WebSocketClientMessage::Subscribe(subscribe) => {
            Ok(InitializedWebSocketInput::Subscribe(subscribe))
        }
        WebSocketClientMessage::Complete(operation_id) => {
            Ok(InitializedWebSocketInput::Complete(operation_id))
        }
        WebSocketClientMessage::Ping(payload) => {
            controls
                .enqueue_peer_pong(&payload)
                .map_err(|_| fail_framework(observer, terminal))?;
            Ok(InitializedWebSocketInput::ControlQueued)
        }
        WebSocketClientMessage::Pong(_) => Ok(InitializedWebSocketInput::PongObserved),
    }
}

pub(super) fn receive_binary(
    terminal: &mut Option<WebSocketConnectionClose>,
) -> WebSocketConnectionClose {
    request_close(terminal, WebSocketConnectionClose::BAD_REQUEST)
}

pub(super) fn fail_framework(
    observer: &Arc<dyn AuthorizationErrorObserver>,
    terminal: &mut Option<WebSocketConnectionClose>,
) -> WebSocketConnectionClose {
    observer.observe(&AuthorizationFrameworkFailure::Control);
    request_close(terminal, WebSocketConnectionClose::INTERNAL)
}

pub(super) fn request_close(
    terminal: &mut Option<WebSocketConnectionClose>,
    close: WebSocketConnectionClose,
) -> WebSocketConnectionClose {
    *terminal.get_or_insert(close)
}
