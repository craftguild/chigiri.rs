//! Client-safe GraphQL responses for transport decoding failures.

use chigiri_core::{GraphQlError, GraphQlResponse};

use crate::{ResponseMediaType, ResponseParts, encode_response_with_status};

/// Failure to decode a GraphQL-over-HTTP request.
#[derive(Clone, Debug, PartialEq)]
pub struct CodecError {
    status: u16,
    response: Box<GraphQlResponse>,
    media_type: ResponseMediaType,
    allow: Option<&'static str>,
}

impl CodecError {
    pub(crate) fn malformed_json(media_type: ResponseMediaType) -> Self {
        Self::new(400, media_type, "Malformed JSON request", "BAD_REQUEST")
    }

    pub(crate) fn malformed_request(media_type: ResponseMediaType, message: &'static str) -> Self {
        Self::new(400, media_type, message, "BAD_REQUEST")
    }

    pub(crate) fn invalid_request(media_type: ResponseMediaType, message: &'static str) -> Self {
        Self::new(422, media_type, message, "BAD_REQUEST")
    }

    pub(crate) fn malformed_uri(media_type: ResponseMediaType, message: &'static str) -> Self {
        Self::new(400, media_type, message, "BAD_REQUEST")
    }

    pub(crate) fn method_not_allowed(media_type: ResponseMediaType) -> Self {
        Self::new(
            405,
            media_type,
            "Only GET and POST requests are supported",
            "METHOD_NOT_ALLOWED",
        )
        .with_allow("GET, POST")
    }

    pub(crate) fn streaming_method_not_allowed() -> Self {
        Self::new(
            405,
            ResponseMediaType::GraphQlResponseJson,
            "HTTP subscriptions require POST",
            "METHOD_NOT_ALLOWED",
        )
        .with_allow("POST")
    }

    pub(crate) fn unsupported_media(media_type: ResponseMediaType) -> Self {
        Self::new(
            415,
            media_type,
            "Content-Type must be application/json with UTF-8 encoding",
            "UNSUPPORTED_MEDIA_TYPE",
        )
    }

    pub(crate) fn not_acceptable() -> Self {
        Self::new(
            406,
            ResponseMediaType::GraphQlResponseJson,
            "No supported response media type is acceptable",
            "NOT_ACCEPTABLE",
        )
    }

    fn new(
        status: u16,
        media_type: ResponseMediaType,
        message: &'static str,
        code: &'static str,
    ) -> Self {
        Self {
            status,
            response: Box::new(GraphQlResponse::request_error(
                GraphQlError::new(message).code(code),
            )),
            media_type,
            allow: None,
        }
    }

    fn with_allow(mut self, allow: &'static str) -> Self {
        self.allow = Some(allow);
        self
    }

    /// Returns the HTTP status.
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the client-safe GraphQL response.
    pub fn response(&self) -> &GraphQlResponse {
        self.response.as_ref()
    }

    /// Returns the response media type selected when it fails.
    pub const fn media_type(&self) -> ResponseMediaType {
        self.media_type
    }

    /// The transport adapter converts it into the response parts.
    pub fn into_response_parts(self) -> ResponseParts {
        let mut response =
            encode_response_with_status(self.media_type, self.status, self.response.as_ref())
                .unwrap_or_else(|_| ResponseParts::serialization_failure(self.media_type));
        if let Some(allow) = self.allow {
            response.push_header("allow", allow);
        }
        response
    }
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "GraphQL over HTTP decode failed with {}",
            self.status
        )
    }
}

impl std::error::Error for CodecError {}
