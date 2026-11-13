use crate::JsonConversionError;
use crate::response_writer::BoundedWriter;
use crate::subscription::SubscriptionTransport;

use super::SubscriptionEventEncodingError;

pub(super) enum EventFrame<'a> {
    WebSocket(&'a str),
    ServerSentEvents,
    Multipart,
}

impl<'a> EventFrame<'a> {
    pub(super) fn select(
        transport: SubscriptionTransport,
        websocket_operation_id: Option<&'a str>,
    ) -> Result<Self, SubscriptionEventEncodingError> {
        match (transport, websocket_operation_id) {
            (SubscriptionTransport::WebSocket, Some(operation_id)) => {
                Ok(Self::WebSocket(operation_id))
            }
            (SubscriptionTransport::ServerSentEvents, None) => Ok(Self::ServerSentEvents),
            (SubscriptionTransport::Multipart, None) => Ok(Self::Multipart),
            _ => Err(SubscriptionEventEncodingError::InvalidTransportArguments),
        }
    }

    pub(super) fn write_prefix(
        &self,
        output: &mut BoundedWriter,
    ) -> Result<(), JsonConversionError> {
        match self {
            Self::WebSocket(operation_id) => {
                output.bytes(b"{\"id\":")?;
                output.string(operation_id)?;
                output.bytes(b",\"type\":\"next\",\"payload\":")
            }
            Self::ServerSentEvents => output.bytes(b"event: next\ndata: "),
            Self::Multipart => {
                output.bytes(b"--graphql\r\nContent-Type: application/json\r\n\r\n{\"payload\":")
            }
        }
    }

    pub(super) fn write_suffix(
        &self,
        output: &mut BoundedWriter,
    ) -> Result<(), JsonConversionError> {
        match self {
            Self::WebSocket(_) => output.byte(b'}'),
            Self::ServerSentEvents => output.bytes(b"\n\n"),
            Self::Multipart => output.bytes(b"}\r\n"),
        }
    }
}
