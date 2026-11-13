//! Bounded encoding for one subscription control or terminal entry.

mod frames;
mod initial;

pub(crate) use initial::encode as encode_http_initial;

use std::fmt;

use chigiri_core::{GraphQlResponse, Object};

use super::SubscriptionTransport;
use super::client_message::{RejectedWebSocketOperationId, WebSocketControlPayload};
use super::event_encoding::{FinalizedSubscriptionEvent, SubscriptionEventEncodingError};
use crate::JsonConversionError;
use crate::response_writer::BoundedWriter;

const HARD_CONTROL_BYTE_LIMIT: usize = 64 * 1024;
const MAX_WEBSOCKET_OPERATION_ID_BYTES: usize = 256;

/// One control or terminal entry to encode for a subscription transport.
///
/// Invalid transport, operation-ID, and delivery-counter combinations fail
/// closed instead of silently dropping inputs.
#[doc(hidden)]
pub enum SubscriptionControl<'a> {
    /// The sole WebSocket `connection_ack` entry.
    ConnectionAck,
    /// A transport-specific heartbeat entry.
    Heartbeat,
    /// A natural completion entry.
    Complete {
        /// Required only for WebSocket operation completion.
        websocket_operation_id: Option<&'a str>,
    },
    /// A WebSocket pong with an optional echoed object payload.
    Pong {
        /// The object copied from a peer ping, or `None` for no payload member.
        payload: Option<&'a Object>,
    },
    /// A terminal operation or transport error entry.
    Error {
        /// Required only for a WebSocket operation error.
        websocket_operation_id: Option<&'a str>,
        /// A client-safe response containing at least one error.
        response: &'a GraphQlResponse,
        /// Delivery metadata used only by the SSE next-error entry.
        dropped_total: Option<u64>,
    },
}

impl fmt::Debug for SubscriptionControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::ConnectionAck => "ConnectionAck",
            Self::Heartbeat => "Heartbeat",
            Self::Complete { .. } => "Complete",
            Self::Pong { .. } => "Pong",
            Self::Error { .. } => "Error",
        };
        formatter
            .debug_tuple("SubscriptionControl")
            .field(&name)
            .finish()
    }
}

/// A static control whose exact encoded length is known before encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SubscriptionStaticControl {
    /// WebSocket connection acknowledgement.
    ConnectionAck,
    /// Transport-specific heartbeat.
    Heartbeat,
    /// SSE or multipart natural completion.
    Complete,
}

enum ControlBytes {
    Owned(Vec<u8>),
    SseEvent(FinalizedSubscriptionEvent),
}

/// Immutable, move-only bytes for one control or terminal entry.
///
/// This owner exposes no mutable buffer and must remain alive until the
/// transport has physically finished writing all charged bytes.
#[doc(hidden)]
pub struct EncodedSubscriptionControl {
    bytes: ControlBytes,
}

impl EncodedSubscriptionControl {
    /// Returns the exact charged byte length, including all transport framing.
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Returns whether this entry occupies no bytes.
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    /// Borrows immutable bytes for the transport write.
    pub fn as_bytes(&self) -> &[u8] {
        match &self.bytes {
            ControlBytes::Owned(bytes) => bytes,
            ControlBytes::SseEvent(event) => event.as_bytes(),
        }
    }

    fn owned(bytes: Vec<u8>) -> Self {
        Self {
            bytes: ControlBytes::Owned(bytes),
        }
    }

    fn sse_event(event: FinalizedSubscriptionEvent) -> Self {
        Self {
            bytes: ControlBytes::SseEvent(event),
        }
    }
}

impl AsRef<[u8]> for EncodedSubscriptionControl {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for EncodedSubscriptionControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncodedSubscriptionControl")
            .field("bytes", &self.len())
            .finish_non_exhaustive()
    }
}

/// A safe failure while encoding one bounded control or terminal entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SubscriptionControlEncodingError {
    /// The shared bounded JSON writer rejected the entry.
    Json(JsonConversionError),
    /// The SSE event encoder rejected its terminal next-error entry.
    Event(SubscriptionEventEncodingError),
    /// The selected control cannot be used with this transport or input set.
    InvalidTransportArguments,
    /// A required WebSocket operation ID is empty or exceeds 256 bytes.
    InvalidWebSocketOperationId,
    /// A terminal error input contains no GraphQL errors.
    MissingErrors,
}

impl fmt::Display for SubscriptionControlEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => error.fmt(formatter),
            Self::Event(error) => error.fmt(formatter),
            Self::InvalidTransportArguments => {
                formatter.write_str("subscription control arguments are invalid")
            }
            Self::InvalidWebSocketOperationId => {
                formatter.write_str("WebSocket operation ID is invalid")
            }
            Self::MissingErrors => {
                formatter.write_str("subscription terminal error payload is empty")
            }
        }
    }
}

impl std::error::Error for SubscriptionControlEncodingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Event(error) => Some(error),
            _ => None,
        }
    }
}

impl From<JsonConversionError> for SubscriptionControlEncodingError {
    fn from(error: JsonConversionError) -> Self {
        Self::Json(error)
    }
}

impl From<SubscriptionEventEncodingError> for SubscriptionControlEncodingError {
    fn from(error: SubscriptionEventEncodingError) -> Self {
        Self::Event(error)
    }
}

/// Returns an exact reservation size for a supported static control entry.
pub(crate) fn static_len(
    transport: SubscriptionTransport,
    control: SubscriptionStaticControl,
) -> Result<usize, SubscriptionControlEncodingError> {
    frames::static_bytes(transport, control)
        .map(<[u8]>::len)
        .ok_or(SubscriptionControlEncodingError::InvalidTransportArguments)
}

/// Encodes exactly one entry within both the caller limit and the intrinsic
/// 64-KiB control-frame ceiling. This function does not reserve bytes: callers
/// must obtain the complete candidate reservation before invoking it.
pub(crate) fn encode(
    transport: SubscriptionTransport,
    control: SubscriptionControl<'_>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let maximum_bytes = maximum_bytes.min(HARD_CONTROL_BYTE_LIMIT);
    match control {
        SubscriptionControl::ConnectionAck => encode_static(
            transport,
            SubscriptionStaticControl::ConnectionAck,
            maximum_bytes,
        ),
        SubscriptionControl::Heartbeat => encode_static(
            transport,
            SubscriptionStaticControl::Heartbeat,
            maximum_bytes,
        ),
        SubscriptionControl::Complete {
            websocket_operation_id,
        } => encode_complete(transport, websocket_operation_id, maximum_bytes),
        SubscriptionControl::Pong { payload } => encode_pong(transport, payload, maximum_bytes),
        SubscriptionControl::Error {
            websocket_operation_id,
            response,
            dropped_total,
        } => encode_error(
            transport,
            websocket_operation_id,
            response,
            dropped_total,
            maximum_bytes,
        ),
    }
}

/// Encodes one operation error for an otherwise valid rejected oversized ID.
///
/// This dedicated path is the only encoder allowed to echo a decoded ID that
/// exceeds the normal 256-byte operation-ID limit. It remains bounded by the
/// intrinsic 64-KiB control-frame ceiling and exposes no general validator
/// bypass for accepted operation IDs.
pub(crate) fn encode_rejected_operation_error(
    operation_id: &RejectedWebSocketOperationId,
    response: &GraphQlResponse,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    if response.errors.is_empty() {
        return Err(SubscriptionControlEncodingError::MissingErrors);
    }
    frames::websocket_error(
        operation_id.as_str(),
        &response.errors,
        maximum_bytes.min(HARD_CONTROL_BYTE_LIMIT),
    )
}

/// Returns the exact framed length of one decoder-owned peer pong.
pub(crate) fn peer_pong_len(payload: &WebSocketControlPayload) -> usize {
    frames::peer_pong_len(payload.raw_object())
}

/// Encodes one echoed peer pong without normalizing or expanding its bounded
/// decoder-owned JSON object.
pub(crate) fn encode_peer_pong(
    payload: &WebSocketControlPayload,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    frames::websocket_peer_pong(
        payload.raw_object(),
        maximum_bytes.min(HARD_CONTROL_BYTE_LIMIT),
    )
}

fn encode_static(
    transport: SubscriptionTransport,
    control: SubscriptionStaticControl,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    let bytes = frames::static_bytes(transport, control)
        .ok_or(SubscriptionControlEncodingError::InvalidTransportArguments)?;
    let mut output = BoundedWriter::new(maximum_bytes);
    output.bytes(bytes)?;
    Ok(EncodedSubscriptionControl::owned(output.finish()))
}

fn encode_complete(
    transport: SubscriptionTransport,
    websocket_operation_id: Option<&str>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    match (transport, websocket_operation_id) {
        (SubscriptionTransport::WebSocket, Some(operation_id)) => {
            validate_operation_id(operation_id)?;
            frames::websocket_complete(operation_id, maximum_bytes)
        }
        (SubscriptionTransport::ServerSentEvents | SubscriptionTransport::Multipart, None) => {
            encode_static(
                transport,
                SubscriptionStaticControl::Complete,
                maximum_bytes,
            )
        }
        _ => Err(SubscriptionControlEncodingError::InvalidTransportArguments),
    }
}

fn encode_pong(
    transport: SubscriptionTransport,
    payload: Option<&Object>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    if transport != SubscriptionTransport::WebSocket {
        return Err(SubscriptionControlEncodingError::InvalidTransportArguments);
    }
    frames::websocket_pong(payload, maximum_bytes)
}

fn encode_error(
    transport: SubscriptionTransport,
    websocket_operation_id: Option<&str>,
    response: &GraphQlResponse,
    dropped_total: Option<u64>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    if response.errors.is_empty() {
        return Err(SubscriptionControlEncodingError::MissingErrors);
    }
    match (transport, websocket_operation_id, dropped_total) {
        (SubscriptionTransport::WebSocket, Some(operation_id), None) => {
            validate_operation_id(operation_id)?;
            frames::websocket_error(operation_id, &response.errors, maximum_bytes)
        }
        (SubscriptionTransport::ServerSentEvents, None, dropped_total) => {
            let event = super::event_encoding::encode(transport, None, response, maximum_bytes)?
                .finalize(dropped_total)?;
            Ok(EncodedSubscriptionControl::sse_event(event))
        }
        (SubscriptionTransport::Multipart, None, None) => {
            frames::multipart_error(&response.errors, maximum_bytes)
        }
        _ => Err(SubscriptionControlEncodingError::InvalidTransportArguments),
    }
}

fn validate_operation_id(operation_id: &str) -> Result<(), SubscriptionControlEncodingError> {
    if operation_id.is_empty() || operation_id.len() > MAX_WEBSOCKET_OPERATION_ID_BYTES {
        return Err(SubscriptionControlEncodingError::InvalidWebSocketOperationId);
    }
    Ok(())
}
