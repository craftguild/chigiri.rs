//! Bounded decoding of one `graphql-transport-ws` client text message.

mod error;
mod payload;
mod raw;
mod types;

pub use error::{
    RejectedWebSocketOperationId, WebSocketClientMessageError, WebSocketClientMessageErrorKind,
};
pub use types::{
    WebSocketClientMessage, WebSocketControlPayload, WebSocketObjectPayload, WebSocketOperationId,
    WebSocketSubscribe,
};

use raw::RawMessage;

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_OPERATION_ID_BYTES: usize = 256;

pub(crate) fn decode(text: &str) -> Result<WebSocketClientMessage, WebSocketClientMessageError> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(WebSocketClientMessageError::message_too_large());
    }
    let message = raw::message(text).map_err(|()| WebSocketClientMessageError::malformed())?;
    match message.kind.as_str() {
        "connection_init" => decode_object_message(message, ObjectMessage::ConnectionInit),
        "ping" => decode_object_message(message, ObjectMessage::Ping),
        "pong" => decode_object_message(message, ObjectMessage::Pong),
        "subscribe" => decode_subscribe(message),
        "complete" => decode_complete(message),
        _ => Err(WebSocketClientMessageError::malformed()),
    }
}

#[derive(Clone, Copy)]
enum ObjectMessage {
    ConnectionInit,
    Ping,
    Pong,
}

fn decode_object_message(
    message: RawMessage<'_>,
    kind: ObjectMessage,
) -> Result<WebSocketClientMessage, WebSocketClientMessageError> {
    match kind {
        ObjectMessage::ConnectionInit => payload::authorization_object(message.payload)
            .map(WebSocketClientMessage::ConnectionInit),
        ObjectMessage::Ping => {
            payload::control_object(message.payload).map(WebSocketClientMessage::Ping)
        }
        ObjectMessage::Pong => {
            payload::control_object(message.payload).map(WebSocketClientMessage::Pong)
        }
    }
}

fn decode_subscribe(
    message: RawMessage<'_>,
) -> Result<WebSocketClientMessage, WebSocketClientMessageError> {
    let operation_id = required_operation_id(message.id)?;
    let raw_payload = message
        .payload
        .ok_or_else(WebSocketClientMessageError::malformed)?;
    let envelope = payload::subscribe(raw_payload)?;
    let operation_id = accepted_operation_id(operation_id)?;
    Ok(WebSocketClientMessage::Subscribe(WebSocketSubscribe::new(
        operation_id,
        envelope,
    )))
}

fn decode_complete(
    message: RawMessage<'_>,
) -> Result<WebSocketClientMessage, WebSocketClientMessageError> {
    let operation_id = required_operation_id(message.id)?;
    Ok(WebSocketClientMessage::Complete(accepted_operation_id(
        operation_id,
    )?))
}

fn required_operation_id(
    raw: Option<&serde_json::value::RawValue>,
) -> Result<String, WebSocketClientMessageError> {
    let raw = raw.ok_or_else(WebSocketClientMessageError::malformed)?;
    let operation_id = serde_json::from_str::<String>(raw.get())
        .map_err(|_| WebSocketClientMessageError::malformed())?;
    if operation_id.is_empty() {
        return Err(WebSocketClientMessageError::malformed());
    }
    Ok(operation_id)
}

fn accepted_operation_id(
    operation_id: String,
) -> Result<WebSocketOperationId, WebSocketClientMessageError> {
    if operation_id.len() > MAX_OPERATION_ID_BYTES {
        return Err(WebSocketClientMessageError::operation_id_too_large(
            operation_id,
        ));
    }
    Ok(WebSocketOperationId::new(operation_id))
}
