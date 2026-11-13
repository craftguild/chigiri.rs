//! Decoding transport-neutral HTTP request parts into GraphQL requests.

mod streaming;

pub use streaming::StreamingHttpPreflight;
pub(crate) use streaming::{decode_streaming_envelope, preflight_streaming_http};

use std::fmt;

use chigiri_core::{GraphQlRequest, Object, Value};

use crate::form;
use crate::json::{ObjectParseError, parse_object, parse_object_text};
use crate::media::{accepts_json_content_type, negotiate_response_media};
use crate::{CodecError, PersistedOperationReference, RequestEnvelope, ResponseMediaType};

/// Request header borrowed from a server adapter.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RequestHeader<'a> {
    /// ASCII header name.
    pub name: &'a str,
    /// Parser verified header value bytes.
    pub value: &'a [u8],
}

impl fmt::Debug for RequestHeader<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestHeader")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

impl<'a> RequestHeader<'a> {
    /// Creates a borrowed header from its name and value.
    pub const fn new(name: &'a str, value: &'a [u8]) -> Self {
        Self { name, value }
    }
}

/// HTTP request parts that are separate from the body stream.
#[derive(Clone, Copy)]
pub struct RequestParts<'a> {
    /// HTTP method token.
    pub method: &'a str,
    /// Optional URI query string.
    pub uri_query: Option<&'a str>,
    /// request headers.
    pub headers: &'a [RequestHeader<'a>],
    /// Body bytes collected up to the configured limit.
    pub body: &'a [u8],
}

impl fmt::Debug for RequestParts<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestParts")
            .field("method", &self.method)
            .field("has_uri_query", &self.uri_query.is_some())
            .field("header_count", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// HTTP method class understood by the codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestMethod {
    /// Permit only query operations, as required for safe HTTP GET handling.
    Get,
    /// Query and Mutation, which allows POST.
    Post,
}

/// Decoded GraphQL request and transport policy.
#[derive(Clone, PartialEq)]
pub struct DecodedRequest {
    /// Request to the core executor.
    pub request: GraphQlRequest,
    /// The HTTP method that constrains which operation kinds may execute.
    pub method: RequestMethod,
    /// negotiated response media type.
    pub response_media_type: ResponseMediaType,
}

/// Request envelope and transport policy before source resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedEnvelope {
    pub envelope: RequestEnvelope,
    pub method: RequestMethod,
    pub response_media_type: ResponseMediaType,
}

impl fmt::Debug for DecodedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedRequest")
            .field("request", &self.request)
            .field("method", &self.method)
            .field("response_media_type", &self.response_media_type)
            .finish()
    }
}

/// Result of validating the HTTP method and negotiating a response before body reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestPreflight {
    /// The HTTP method that constrains which operation kinds may execute.
    pub method: RequestMethod,
    /// negotiated response media type.
    pub response_media_type: ResponseMediaType,
    /// Whether the request body requires JSON decoding.
    pub body_required: bool,
}

/// Validates method and headers so invalid or unnecessary bodies need not be read.
pub fn preflight_request(
    method: &str,
    headers: &[RequestHeader<'_>],
) -> Result<RequestPreflight, CodecError> {
    let response_media_type = negotiate_response_media(headers)?;
    let method = match method {
        "GET" => RequestMethod::Get,
        "POST" => RequestMethod::Post,
        _ => return Err(CodecError::method_not_allowed(response_media_type)),
    };
    if method == RequestMethod::Post && !accepts_json_content_type(headers) {
        return Err(CodecError::unsupported_media(response_media_type));
    }
    Ok(RequestPreflight {
        method,
        response_media_type,
        body_required: method == RequestMethod::Post,
    })
}

/// Decodes HTTP request parts into a well-formed GraphQL request.
pub fn decode_request(parts: RequestParts<'_>) -> Result<DecodedRequest, CodecError> {
    let decoded = decode_envelope(parts)?;
    let media = decoded.response_media_type;
    let request = decoded
        .envelope
        .into_document_request()
        .ok_or_else(|| CodecError::invalid_request(media, "The query member is required"))?;
    Ok(DecodedRequest {
        request,
        method: decoded.method,
        response_media_type: media,
    })
}

/// Decodes an HTTP request into an envelope whose document may remain unresolved.
pub fn decode_envelope(parts: RequestParts<'_>) -> Result<DecodedEnvelope, CodecError> {
    let preflight = preflight_request(parts.method, parts.headers)?;
    let envelope = match preflight.method {
        RequestMethod::Get => decode_get_envelope(parts.uri_query, preflight.response_media_type)?,
        RequestMethod::Post => decode_post_envelope(parts.body, preflight.response_media_type)?,
    };
    Ok(DecodedEnvelope {
        envelope,
        method: preflight.method,
        response_media_type: preflight.response_media_type,
    })
}

fn decode_post_envelope(
    body: &[u8],
    media: ResponseMediaType,
) -> Result<RequestEnvelope, CodecError> {
    let object = parse_object(body).map_err(|error| match error {
        ObjectParseError::InvalidJson => CodecError::malformed_json(media),
        ObjectParseError::InvalidObject => {
            CodecError::invalid_request(media, "The request body must be a JSON object")
        }
    })?;
    envelope_from_object(object, media)
}

fn decode_get_envelope(
    uri_query: Option<&str>,
    media: ResponseMediaType,
) -> Result<RequestEnvelope, CodecError> {
    let parameters = form::decode(uri_query.unwrap_or_default())
        .map_err(|_| CodecError::malformed_uri(media, "Malformed URL query"))?;
    let query = parameters.get("query").cloned();
    let operation_name = parameters
        .get("operationName")
        .filter(|value| !value.is_empty())
        .cloned();
    let variables = get_json_object(&parameters, "variables", media)?;
    let extensions = get_json_object(&parameters, "extensions", media)?;
    envelope_from_parts(query, operation_name, variables, extensions, media)
}

fn get_json_object(
    parameters: &std::collections::BTreeMap<String, String>,
    name: &str,
    media: ResponseMediaType,
) -> Result<Object, CodecError> {
    let Some(value) = parameters.get(name).filter(|value| !value.is_empty()) else {
        return Ok(Object::new());
    };
    parse_object_text(value)
        .map_err(|_| CodecError::invalid_request(media, "Malformed JSON parameter"))
}

pub(crate) fn envelope_from_object(
    mut object: Object,
    media: ResponseMediaType,
) -> Result<RequestEnvelope, CodecError> {
    let query = take_string(&mut object, "query", false)
        .map_err(|_| CodecError::invalid_request(media, "The query member must be a string"))?;
    let operation_name = take_string(&mut object, "operationName", true).map_err(|_| {
        CodecError::invalid_request(media, "operationName must be a string or null")
    })?;
    let variables = take_object(&mut object, "variables")
        .map_err(|_| CodecError::invalid_request(media, "variables must be an object or null"))?;
    let extensions = take_object(&mut object, "extensions")
        .map_err(|_| CodecError::invalid_request(media, "extensions must be an object or null"))?;
    envelope_from_parts(query, operation_name, variables, extensions, media)
}

fn envelope_from_parts(
    query: Option<String>,
    operation_name: Option<String>,
    variables: Object,
    extensions: Object,
    media: ResponseMediaType,
) -> Result<RequestEnvelope, CodecError> {
    let persisted = extract_persisted_reference(&extensions, media)?;
    match persisted {
        Some(reference) => Ok(RequestEnvelope::persisted(
            query,
            operation_name,
            variables,
            extensions,
            reference,
        )),
        None => query
            .map(|query| {
                RequestEnvelope::document(GraphQlRequest {
                    query,
                    operation_name,
                    variables,
                    extensions,
                    catalog_cost_hint: None,
                    telemetry: None,
                })
            })
            .ok_or_else(|| CodecError::invalid_request(media, "The query member is required")),
    }
}

fn extract_persisted_reference(
    extensions: &Object,
    media: ResponseMediaType,
) -> Result<Option<PersistedOperationReference>, CodecError> {
    let Some(value) = extensions.get("persistedQuery") else {
        return Ok(None);
    };
    let Some(persisted) = value.as_object() else {
        return Err(invalid_persisted(media));
    };
    if persisted.get("version") != Some(&Value::Int(1)) {
        return Err(invalid_persisted(media));
    }
    let Some(hash) = persisted.get("sha256Hash").and_then(Value::as_str) else {
        return Err(invalid_persisted(media));
    };
    PersistedOperationReference::apollo_apq_v1(hash.to_owned())
        .map(Some)
        .map_err(|_| invalid_persisted(media))
}

fn invalid_persisted(media: ResponseMediaType) -> CodecError {
    CodecError::malformed_request(media, "Malformed persisted operation extension")
}

fn take_string(object: &mut Object, name: &str, nullable: bool) -> Result<Option<String>, ()> {
    match object.remove(name) {
        None => Ok(None),
        Some(Value::Null) if nullable => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(()),
    }
}

fn take_object(object: &mut Object, name: &str) -> Result<Object, ()> {
    match object.remove(name) {
        None | Some(Value::Null) => Ok(Object::new()),
        Some(Value::Object(value)) => Ok(value),
        _ => Err(()),
    }
}
