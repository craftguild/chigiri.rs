//! Deferred HTTP response negotiation before an operation kind is known.

use crate::media::{accepts_json_content_type, negotiate_response_media};
use crate::{
    CodecError, DecodedEnvelope, RequestHeader, RequestMethod, ResponseMediaType,
    SubscriptionTransport,
};

/// Method, body, and bounded response candidates for streaming-aware ingress.
///
/// This value borrows and retains no request headers. The finite and streaming
/// candidates remain provisional until the resolved GraphQL source identifies
/// one selected operation.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamingHttpPreflight {
    method: RequestMethod,
    body_required: bool,
    provisional_media: ResponseMediaType,
    finite: Result<ResponseMediaType, CodecError>,
    streaming: Result<SubscriptionTransport, CodecError>,
}

impl StreamingHttpPreflight {
    /// Returns the accepted GraphQL-over-HTTP method class.
    pub const fn method(&self) -> RequestMethod {
        self.method
    }

    /// Returns whether ingress must collect and decode a JSON request body.
    pub const fn body_required(&self) -> bool {
        self.body_required
    }

    /// Returns the finite media used for failures before operation selection.
    pub const fn provisional_media(&self) -> ResponseMediaType {
        self.provisional_media
    }

    /// Returns the ordinary finite response candidate.
    pub fn finite(&self) -> Result<ResponseMediaType, CodecError> {
        self.finite.clone()
    }

    /// Returns the already-selected Subscription response candidate.
    pub fn streaming(&self) -> Result<SubscriptionTransport, CodecError> {
        self.streaming.clone()
    }
}

/// Validates common HTTP input while deferring the response candidate choice.
///
/// Method and request Content-Type failures are final before body polling.
/// `Accept` is scanned into both bounded candidates, so a stream-only range is
/// not rejected before the resolved operation kind is known.
pub fn preflight_streaming_http(
    method: &str,
    headers: &[RequestHeader<'_>],
) -> Result<StreamingHttpPreflight, CodecError> {
    let finite = negotiate_response_media(headers);
    let provisional_media = finite
        .as_ref()
        .copied()
        .unwrap_or(ResponseMediaType::GraphQlResponseJson);
    let method = match method {
        "GET" => RequestMethod::Get,
        "POST" => RequestMethod::Post,
        _ => return Err(CodecError::method_not_allowed(provisional_media)),
    };
    if method == RequestMethod::Post && !accepts_json_content_type(headers) {
        return Err(CodecError::unsupported_media(provisional_media));
    }
    let streaming = crate::subscription::negotiate_http_profile(
        match method {
            RequestMethod::Get => "GET",
            RequestMethod::Post => "POST",
        },
        headers,
    );
    Ok(StreamingHttpPreflight {
        method,
        body_required: method == RequestMethod::Post,
        provisional_media,
        finite,
        streaming,
    })
}

/// Decodes a request envelope without repeating method or `Accept` policy.
///
/// The returned response media is provisional. The streaming-aware facade
/// finalizes one candidate only after source resolution and decoded hooks.
pub fn decode_streaming_envelope(
    preflight: &StreamingHttpPreflight,
    uri_query: Option<&str>,
    body: &[u8],
) -> Result<DecodedEnvelope, CodecError> {
    let envelope = match preflight.method {
        RequestMethod::Get => super::decode_get_envelope(uri_query, preflight.provisional_media)?,
        RequestMethod::Post => super::decode_post_envelope(body, preflight.provisional_media)?,
    };
    Ok(DecodedEnvelope {
        envelope,
        method: preflight.method,
        response_media_type: preflight.provisional_media,
    })
}
