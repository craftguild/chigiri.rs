//! Transport contracts independent of any server library.

mod body;
mod csrf;
mod deadline;
mod envelope;
mod error;
mod form;
mod headers;
mod hooks;
mod instrumentation;
mod json;
mod lifecycle;
mod media;
mod pipeline;
mod request;
mod response;
mod response_writer;
mod settings;
mod subscription;
mod transport;

mod completion;
pub use body::{BodyReadError, BufferedBody, RequestBody};
pub use chigiri_core::TraceId;
pub use completion::{ResponseCompletion, ResponseCompletionOutcome};
pub use csrf::{CsrfConfigurationError, CsrfProtection};
pub use deadline::DeadlineFactory;
pub use envelope::{
    EnvelopeError, PersistedOperationProtocol, PersistedOperationReference, RequestEnvelope,
};
pub use error::CodecError;
pub use headers::{ResponseHeaderError, ResponseHeaders};
pub use hooks::{
    ContextFactory, HookError, PrepareError, PreparedContext, RequestContext, ResponseHook,
    ResponseHookContext,
};
pub use json::{JsonConversionError, value_from_json, value_to_json};
pub use lifecycle::{
    AdmissionDecision, AdmissionError, BoxFuture, DecodedRequestHook, DecodedRequestView,
    OperationSourceResolver, Rejection, RejectionError, ResolvedOperationSource, RetryAfter,
    SourceResolutionError, TransportAdmission,
};
pub use media::{ResponseMediaType, negotiate_response_media};
pub use pipeline::HttpLifecycle;
#[doc(hidden)]
pub use request::StreamingHttpPreflight;
pub use request::{
    DecodedEnvelope, DecodedRequest, RequestHeader, RequestMethod, RequestParts, RequestPreflight,
    decode_envelope, decode_request, preflight_request,
};
pub use response::{ResponseHeader, ResponseParts, encode_response, encode_response_with_status};
pub use subscription::{
    AcceptConnectionInit, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError, EncodedSubscriptionControl, EncodedSubscriptionEvent,
    FinalizedSubscriptionEvent, RejectedWebSocketOperationId, RequestPhase, SubscriptionControl,
    SubscriptionControlEncodingError, SubscriptionEventEncodingError, SubscriptionStaticControl,
    SubscriptionTransport, WebSocketClientMessage, WebSocketClientMessageError,
    WebSocketClientMessageErrorKind, WebSocketControlPayload, WebSocketObjectPayload,
    WebSocketOneShot, WebSocketOperationId, WebSocketSubscribe,
};
#[doc(hidden)]
pub use subscription::{
    VerifiedWebSocketUpgrade, WebSocketAcceptedTextProgress, WebSocketHttpVersion,
    WebSocketUpgradeError, WebSocketWriteProgressError, WebSocketWriteProgressTracker,
};
pub use transport::{
    ApplicationMetadata, MetadataError, MetadataField, TransportRequest, TransportUri,
};

/// Checks the frozen HTTP/1.1 WebSocket upgrade profile without reading a body.
///
/// Success is syntax/negotiation only, not transport, origin, CSRF, connection,
/// or initialization admission. Bridges must complete those before replying
/// with 101. The reviewed WebSocket library computes the RFC accept key.
#[doc(hidden)]
pub fn __preflight_websocket_upgrade<'a>(
    method: &str,
    version: WebSocketHttpVersion,
    headers: &[RequestHeader<'a>],
) -> Result<VerifiedWebSocketUpgrade<'a>, WebSocketUpgradeError> {
    subscription::websocket_upgrade::preflight(method, version, headers)
}

/// Checks HTTP input while retaining finite and subscription media candidates.
///
/// This internal entry does not classify an operation or authorize a stream.
#[doc(hidden)]
pub fn __preflight_streaming_http(
    method: &str,
    headers: &[RequestHeader<'_>],
) -> Result<StreamingHttpPreflight, CodecError> {
    request::preflight_streaming_http(method, headers)
}

/// Decodes once using the method bound to the deferred preflight result.
#[doc(hidden)]
pub fn __decode_streaming_envelope(
    preflight: &StreamingHttpPreflight,
    uri_query: Option<&str>,
    body: &[u8],
) -> Result<DecodedEnvelope, CodecError> {
    request::decode_streaming_envelope(preflight, uri_query, body)
}

/// An open-source interchangeable input that shares a limited response encode inside the façade.
#[doc(hidden)]
pub fn __encode_response_with_limit(
    media_type: ResponseMediaType,
    response: &chigiri_runtime::GraphQlResponse,
    maximum_bytes: usize,
) -> Result<ResponseParts, JsonConversionError> {
    response::encode_response_with_limit(media_type, response, maximum_bytes)
}

/// An open-source interactive input that shares a status-bounded response encode inside the façade.
#[doc(hidden)]
pub fn __encode_response_with_status_and_limit(
    media_type: ResponseMediaType,
    status: u16,
    response: &chigiri_runtime::GraphQlResponse,
    maximum_bytes: usize,
) -> Result<ResponseParts, JsonConversionError> {
    response::encode_response_with_status_and_limit(media_type, status, response, maximum_bytes)
}

/// Encodes one complete subscription event entry within its byte reservation.
#[doc(hidden)]
pub fn __encode_subscription_event(
    transport: SubscriptionTransport,
    websocket_operation_id: Option<&str>,
    response: &chigiri_runtime::GraphQlResponse,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionEvent, SubscriptionEventEncodingError> {
    subscription::encode_event(transport, websocket_operation_id, response, maximum_bytes)
}

/// Encodes one control or terminal entry within its pre-acquired reservation.
#[doc(hidden)]
pub fn __encode_subscription_control(
    transport: SubscriptionTransport,
    control: SubscriptionControl<'_>,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    subscription::encode_control(transport, control, maximum_bytes)
}

/// Prepares initial SSE/multipart headers and an optional terminal error body.
///
/// The caller must hold the maximum initial-response reservation before calling.
/// The combined body, header-data, and per-entry bookkeeping charge is bounded
/// by both the supplied limit and 64 KiB. Error and completion framing share one
/// writer; a successful live source has an empty initial body.
#[doc(hidden)]
pub fn __encode_subscription_http_initial(
    transport: SubscriptionTransport,
    error: Option<&chigiri_runtime::GraphQlResponse>,
    headers: ResponseHeaders,
    maximum_bytes: usize,
) -> Result<ResponseParts, SubscriptionControlEncodingError> {
    subscription::encode_http_initial(transport, error, headers, maximum_bytes)
}

/// Returns the exact encoded length of one supported static control entry.
#[doc(hidden)]
pub fn __subscription_static_control_len(
    transport: SubscriptionTransport,
    control: SubscriptionStaticControl,
) -> Result<usize, SubscriptionControlEncodingError> {
    subscription::static_control_len(transport, control)
}

/// Negotiates an already selected Subscription's POST-only HTTP stream profile.
///
/// This does not select an operation or change ordinary finite HTTP dispatch.
#[doc(hidden)]
pub fn __negotiate_subscription_http_profile(
    method: &str,
    headers: &[RequestHeader<'_>],
) -> Result<SubscriptionTransport, CodecError> {
    subscription::negotiate_http_profile(method, headers)
}

/// Decodes one complete bounded client text message for the pinned protocol.
#[doc(hidden)]
pub fn __decode_websocket_client_message(
    message: &str,
) -> Result<WebSocketClientMessage, WebSocketClientMessageError> {
    subscription::decode_client_message(message)
}

/// Encodes an identifiable rejected ID without admitting it as an active ID.
///
/// If this error entry cannot fit the reserved control limit, the state owner
/// closes safely instead of allocating a larger candidate or truncating an ID.
#[doc(hidden)]
pub fn __encode_rejected_websocket_operation_error(
    operation_id: &RejectedWebSocketOperationId,
    response: &chigiri_runtime::GraphQlResponse,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    subscription::encode_rejected_operation_error(operation_id, response, maximum_bytes)
}

/// Returns the exact reservation needed to echo a validated peer ping payload.
#[doc(hidden)]
pub fn __websocket_peer_pong_len(payload: &WebSocketControlPayload) -> usize {
    subscription::peer_pong_len(payload)
}

/// Echoes one immutable bounded peer ping payload without numeric expansion.
#[doc(hidden)]
pub fn __encode_websocket_peer_pong(
    payload: &WebSocketControlPayload,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    subscription::encode_peer_pong(payload, maximum_bytes)
}
