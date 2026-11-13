use chigiri_core::{GraphQlError, Object};

use super::{
    EncodedSubscriptionControl, SubscriptionControlEncodingError, SubscriptionStaticControl,
};
use crate::response::{ErrorProjection, write_errors, write_object};
use crate::response_writer::BoundedWriter;
use crate::subscription::SubscriptionTransport;

const WEBSOCKET_ACK: &[u8] = br#"{"type":"connection_ack"}"#;
const WEBSOCKET_HEARTBEAT: &[u8] = br#"{"type":"ping"}"#;
const WEBSOCKET_PONG: &[u8] = br#"{"type":"pong"}"#;
const WEBSOCKET_PONG_PAYLOAD: &[u8] = br#"{"type":"pong","payload":"#;
const SSE_HEARTBEAT: &[u8] = b":\n\n";
const SSE_COMPLETE: &[u8] = b"event: complete\ndata: \n\n";
const MULTIPART_HEARTBEAT: &[u8] = b"--graphql\r\nContent-Type: application/json\r\n\r\n{}\r\n";
const MULTIPART_COMPLETE: &[u8] = b"--graphql--";
const MULTIPART_PREFIX: &[u8] = b"--graphql\r\nContent-Type: application/json\r\n\r\n";

pub(super) fn static_bytes(
    transport: SubscriptionTransport,
    control: SubscriptionStaticControl,
) -> Option<&'static [u8]> {
    match (transport, control) {
        (SubscriptionTransport::WebSocket, SubscriptionStaticControl::ConnectionAck) => {
            Some(WEBSOCKET_ACK)
        }
        (SubscriptionTransport::WebSocket, SubscriptionStaticControl::Heartbeat) => {
            Some(WEBSOCKET_HEARTBEAT)
        }
        (SubscriptionTransport::ServerSentEvents, SubscriptionStaticControl::Heartbeat) => {
            Some(SSE_HEARTBEAT)
        }
        (SubscriptionTransport::ServerSentEvents, SubscriptionStaticControl::Complete) => {
            Some(SSE_COMPLETE)
        }
        (SubscriptionTransport::Multipart, SubscriptionStaticControl::Heartbeat) => {
            Some(MULTIPART_HEARTBEAT)
        }
        (SubscriptionTransport::Multipart, SubscriptionStaticControl::Complete) => {
            Some(MULTIPART_COMPLETE)
        }
        _ => None,
    }
}

pub(super) fn websocket_complete(
    operation_id: &str,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum_bytes);
    output.bytes(b"{\"id\":")?;
    output.string(operation_id)?;
    output.bytes(b",\"type\":\"complete\"}")?;
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

pub(super) fn websocket_pong(
    payload: Option<&Object>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum_bytes);
    output.bytes(b"{\"type\":\"pong\"")?;
    if let Some(payload) = payload {
        output.bytes(b",\"payload\":")?;
        write_object(&mut output, payload)?;
    }
    output.byte(b'}')?;
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

pub(super) fn peer_pong_len(payload: Option<&str>) -> usize {
    payload.map_or(WEBSOCKET_PONG.len(), |payload| {
        WEBSOCKET_PONG_PAYLOAD.len() + payload.len() + 1
    })
}

pub(super) fn websocket_peer_pong(
    payload: Option<&str>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum_bytes);
    match payload {
        Some(payload) => {
            output.bytes(WEBSOCKET_PONG_PAYLOAD)?;
            output.bytes(payload.as_bytes())?;
            output.byte(b'}')?;
        }
        None => output.bytes(WEBSOCKET_PONG)?,
    }
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

pub(super) fn websocket_error(
    operation_id: &str,
    errors: &[GraphQlError],
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum_bytes);
    output.bytes(b"{\"id\":")?;
    output.string(operation_id)?;
    output.bytes(b",\"type\":\"error\",\"payload\":")?;
    write_errors(&mut output, errors, ErrorProjection::GraphQl)?;
    output.byte(b'}')?;
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

pub(super) fn multipart_error(
    errors: &[GraphQlError],
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum_bytes);
    write_multipart_error(&mut output, errors)?;
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

pub(super) fn write_multipart_error(
    output: &mut BoundedWriter,
    errors: &[GraphQlError],
) -> Result<(), SubscriptionControlEncodingError> {
    output.bytes(MULTIPART_PREFIX)?;
    output.bytes(b"{\"payload\":null,\"errors\":")?;
    write_errors(output, errors, ErrorProjection::TopLevelTransport)?;
    output.bytes(b"}\r\n")?;
    Ok(())
}
