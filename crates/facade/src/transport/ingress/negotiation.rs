//! Closed media/decode policy shared by finite and streaming-aware ingress.

use chigiri_http::{
    __decode_streaming_envelope, __preflight_streaming_http, CodecError, DecodedEnvelope,
    RequestMethod, RequestParts, RequestPreflight, ResponseMediaType, StreamingHttpPreflight,
    SubscriptionTransport, TransportRequest, decode_envelope, preflight_request,
};

#[derive(Clone, Copy)]
pub(crate) enum HttpIngressMode {
    Finite,
    StreamingAware,
}

pub(crate) enum HttpNegotiation {
    Finite(RequestPreflight),
    StreamingAware(StreamingHttpPreflight),
}

impl HttpNegotiation {
    pub(super) fn preflight(
        mode: HttpIngressMode,
        request: TransportRequest<'_>,
    ) -> Result<Self, CodecError> {
        match mode {
            HttpIngressMode::Finite => {
                preflight_request(request.method(), request.headers()).map(Self::Finite)
            }
            HttpIngressMode::StreamingAware => {
                __preflight_streaming_http(request.method(), request.headers())
                    .map(Self::StreamingAware)
            }
        }
    }

    pub(crate) fn method(&self) -> RequestMethod {
        match self {
            Self::Finite(value) => value.method,
            Self::StreamingAware(value) => value.method(),
        }
    }

    pub(crate) fn provisional_media(&self) -> ResponseMediaType {
        match self {
            Self::Finite(value) => value.response_media_type,
            Self::StreamingAware(value) => value.provisional_media(),
        }
    }

    pub(super) fn body_required(&self) -> bool {
        match self {
            Self::Finite(value) => value.body_required,
            Self::StreamingAware(value) => value.body_required(),
        }
    }

    pub(crate) fn finite_media(&self) -> Result<ResponseMediaType, CodecError> {
        match self {
            Self::Finite(value) => Ok(value.response_media_type),
            Self::StreamingAware(value) => value.finite(),
        }
    }

    pub(crate) fn subscription_profile(&self) -> Option<Result<SubscriptionTransport, CodecError>> {
        match self {
            Self::Finite(_) => None,
            Self::StreamingAware(value) => Some(value.streaming()),
        }
    }

    pub(super) fn decode(
        &self,
        request: TransportRequest<'_>,
        bytes: &[u8],
    ) -> Result<DecodedEnvelope, CodecError> {
        match self {
            // Preserve the existing finite codec's exact precedence and shape.
            Self::Finite(_) => decode_envelope(RequestParts {
                method: request.method(),
                uri_query: request.uri().query(),
                headers: request.headers(),
                body: bytes,
            }),
            Self::StreamingAware(value) => {
                __decode_streaming_envelope(value, request.uri().query(), bytes)
            }
        }
    }
}
