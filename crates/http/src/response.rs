//! Ordered JSON encoding of core GraphQL responses into HTTP responses.

mod error;

use std::fmt;

use chigiri_core::{GraphQlResponse, Object, Value};

use crate::response_writer::BoundedWriter;
use crate::{JsonConversionError, ResponseMediaType};

pub(crate) use error::{ErrorProjection, write_errors};

/// Response header passed to a server adapter.
#[derive(Clone, Eq, PartialEq)]
pub struct ResponseHeader {
    /// lower-case header name.
    pub name: String,
    /// HTTP field value.
    pub value: String,
}

impl fmt::Debug for ResponseHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseHeader")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

/// HTTP response parts that do not depend on the type of server library.
#[derive(Clone, Eq, PartialEq)]
pub struct ResponseParts {
    /// HTTP status code.
    pub status: u16,
    /// response headers.
    pub headers: Vec<ResponseHeader>,
    /// GraphQL JSON or body bytes of specified fixed UI property.
    pub body: Vec<u8>,
}

impl fmt::Debug for ResponseParts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseParts")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl ResponseParts {
    pub(crate) fn push_header(&mut self, name: &str, value: &str) {
        self.headers.push(ResponseHeader {
            name: name.to_owned(),
            value: value.to_owned(),
        });
    }

    /// The adapter serialization failure is converted to a fixed masked response.
    #[doc(hidden)]
    pub fn serialization_failure(media_type: ResponseMediaType) -> Self {
        Self {
            status: 500,
            headers: vec![ResponseHeader {
                name: "content-type".to_owned(),
                value: media_type.content_type().to_owned(),
            }],
            body: br#"{"errors":[{"message":"Internal server error","extensions":{"code":"INTERNAL_SERVER_ERROR"}}]}"#.to_vec(),
        }
    }
}

/// Selects status and media type from a GraphQL result and encodes the response.
pub fn encode_response(
    media_type: ResponseMediaType,
    response: &GraphQlResponse,
) -> Result<ResponseParts, JsonConversionError> {
    encode_response_with_limit(
        media_type,
        response,
        crate::settings::DEFAULT_RESPONSE_BYTE_LIMIT,
    )
}

pub(crate) fn encode_response_with_limit(
    media_type: ResponseMediaType,
    response: &GraphQlResponse,
    maximum_bytes: usize,
) -> Result<ResponseParts, JsonConversionError> {
    let status = response_status(media_type, response);
    let mut encoded =
        encode_response_with_status_and_limit(media_type, status, response, maximum_bytes)?;
    if status == 405 {
        encoded.push_header("allow", "GET, POST");
    }
    if status == 429
        && let Some(seconds) = retry_after_seconds(response)
    {
        encoded.push_header("retry-after", &seconds.to_string());
    }
    Ok(encoded)
}

fn response_status(media_type: ResponseMediaType, response: &GraphQlResponse) -> u16 {
    if method_not_allowed(response) {
        return 405;
    }
    if response.data.is_none() && has_error_code(response, "RATE_LIMITED") {
        return 429;
    }
    if response.data.is_none() && has_error_code(response, "RATE_LIMIT_UNAVAILABLE") {
        return 503;
    }
    if response.data.is_some() || media_type == ResponseMediaType::Json {
        return 200;
    }
    if response.errors.iter().any(|error| {
        error.extensions.get("code").and_then(Value::as_str) == Some("GRAPHQL_PARSE_FAILED")
    }) {
        400
    } else {
        422
    }
}

fn has_error_code(response: &GraphQlResponse, code: &str) -> bool {
    response
        .errors
        .iter()
        .any(|error| error.extensions.get("code").and_then(Value::as_str) == Some(code))
}

fn retry_after_seconds(response: &GraphQlResponse) -> Option<u64> {
    response.errors.iter().find_map(|error| {
        let Value::Int(value) = error.extensions.get("retryAfterSeconds")? else {
            return None;
        };
        u64::try_from(*value).ok()
    })
}

/// Encodes an adapter-specific failure as a GraphQL response with the given status.
pub fn encode_response_with_status(
    media_type: ResponseMediaType,
    status: u16,
    response: &GraphQlResponse,
) -> Result<ResponseParts, JsonConversionError> {
    encode_response_with_status_and_limit(
        media_type,
        status,
        response,
        crate::settings::DEFAULT_RESPONSE_BYTE_LIMIT,
    )
}

pub(crate) fn encode_response_with_status_and_limit(
    media_type: ResponseMediaType,
    status: u16,
    response: &GraphQlResponse,
    maximum_bytes: usize,
) -> Result<ResponseParts, JsonConversionError> {
    let mut body = BoundedWriter::new(maximum_bytes);
    write_response(&mut body, response)?;
    Ok(ResponseParts {
        status,
        headers: vec![ResponseHeader {
            name: "content-type".to_owned(),
            value: media_type.content_type().to_owned(),
        }],
        body: body.finish(),
    })
}

fn method_not_allowed(response: &GraphQlResponse) -> bool {
    response.data.is_none()
        && response.errors.iter().any(|error| {
            error.extensions.get("code").and_then(Value::as_str) == Some("METHOD_NOT_ALLOWED")
        })
}

fn write_response(
    output: &mut BoundedWriter,
    response: &GraphQlResponse,
) -> Result<(), JsonConversionError> {
    write_response_with_extensions(
        output,
        response,
        !response.extensions.is_empty(),
        write_object,
    )
}

pub(crate) fn write_response_with_extensions<E>(
    output: &mut BoundedWriter,
    response: &GraphQlResponse,
    include_extensions: bool,
    write_extensions: impl FnOnce(&mut BoundedWriter, &Object) -> Result<(), E>,
) -> Result<(), E>
where
    E: From<JsonConversionError>,
{
    output.byte(b'{')?;
    let mut comma = false;
    if let Some(data) = &response.data {
        member(output, &mut comma, "data")?;
        write_value(output, data)?;
    }
    if !response.errors.is_empty() {
        member(output, &mut comma, "errors")?;
        write_errors(output, &response.errors, ErrorProjection::GraphQl)?;
    }
    if include_extensions {
        member(output, &mut comma, "extensions")?;
        write_extensions(output, &response.extensions)?;
    }
    output.byte(b'}')?;
    Ok(())
}

pub(crate) fn write_value(
    output: &mut BoundedWriter,
    value: &Value,
) -> Result<(), JsonConversionError> {
    match value {
        Value::Null => output.bytes(b"null")?,
        Value::Boolean(value) => output.bytes(if *value { b"true" } else { b"false" })?,
        Value::Int(value) => formatted(output, format_args!("{value}"))?,
        Value::BigInt(value) => {
            crate::json::big_int_number(value)?;
            output.bytes(value.as_bytes())?;
        }
        Value::Float(value) if value.is_finite() => formatted(output, format_args!("{value}"))?,
        Value::Float(_) => return Err(JsonConversionError::NonFiniteFloat),
        Value::String(value) => string(output, value)?,
        Value::List(values) => {
            output.byte(b'[')?;
            for (index, value) in values.iter().enumerate() {
                separator(output, index)?;
                write_value(output, value)?;
            }
            output.byte(b']')?;
        }
        Value::Object(value) => write_object(output, value)?,
    }
    Ok(())
}

pub(crate) fn write_object(
    output: &mut BoundedWriter,
    object: &Object,
) -> Result<(), JsonConversionError> {
    output.byte(b'{')?;
    for (index, (name, value)) in object.iter().enumerate() {
        separator(output, index)?;
        string(output, name)?;
        output.byte(b':')?;
        write_value(output, value)?;
    }
    output.byte(b'}')?;
    Ok(())
}

fn member(
    output: &mut BoundedWriter,
    comma: &mut bool,
    name: &str,
) -> Result<(), JsonConversionError> {
    if *comma {
        output.byte(b',')?;
    }
    *comma = true;
    string(output, name)?;
    output.byte(b':')
}

fn separator(output: &mut BoundedWriter, index: usize) -> Result<(), JsonConversionError> {
    if index > 0 {
        output.byte(b',')?;
    }
    Ok(())
}

fn string(output: &mut BoundedWriter, value: &str) -> Result<(), JsonConversionError> {
    output.string(value)
}

fn formatted(
    output: &mut BoundedWriter,
    arguments: std::fmt::Arguments<'_>,
) -> Result<(), JsonConversionError> {
    output.formatted(arguments)
}
