//! One bounded initial HTTP response without separately allocated frame copies.

use chigiri_core::GraphQlResponse;

use super::{HARD_CONTROL_BYTE_LIMIT, SubscriptionControlEncodingError, SubscriptionStaticControl};
use crate::response_writer::BoundedWriter;
use crate::{
    JsonConversionError, ResponseHeader, ResponseHeaders, ResponseParts, SubscriptionTransport,
};

const HEADER_ENTRY_BYTES: usize = 16;
const SSE_CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";
const MULTIPART_CONTENT_TYPE: &str = "multipart/mixed; boundary=graphql; subscriptionSpec=1.0";
const CACHE_CONTROL: &str = "no-cache, no-transform";

pub(crate) fn encode(
    transport: SubscriptionTransport,
    error: Option<&GraphQlResponse>,
    additions: ResponseHeaders,
    maximum_bytes: usize,
) -> Result<ResponseParts, SubscriptionControlEncodingError> {
    let content_type = match transport {
        SubscriptionTransport::ServerSentEvents => SSE_CONTENT_TYPE,
        SubscriptionTransport::Multipart => MULTIPART_CONTENT_TYPE,
        SubscriptionTransport::WebSocket => {
            return Err(SubscriptionControlEncodingError::InvalidTransportArguments);
        }
    };
    if error.is_some_and(|response| response.errors.is_empty()) {
        return Err(SubscriptionControlEncodingError::MissingErrors);
    }
    let maximum = maximum_bytes.min(HARD_CONTROL_BYTE_LIMIT);
    let mut headers = additions.__into_values();
    let mut header_bytes = header_charge("content-type", content_type)?;
    if transport == SubscriptionTransport::ServerSentEvents {
        header_bytes = add(header_bytes, header_charge("cache-control", CACHE_CONTROL)?)?;
    }
    for header in &headers {
        header_bytes = add(header_bytes, header_charge(&header.name, &header.value)?)?;
    }
    let body_limit = maximum
        .checked_sub(header_bytes)
        .ok_or(JsonConversionError::ResponseSizeLimit)?;
    let body = match error {
        Some(response) => encode_error(transport, response, body_limit)?,
        None => Vec::new(),
    };
    // Codec-owned allocation starts only after the complete header charge fits.
    headers.push(ResponseHeader {
        name: "content-type".to_owned(),
        value: content_type.to_owned(),
    });
    if transport == SubscriptionTransport::ServerSentEvents {
        headers.push(ResponseHeader {
            name: "cache-control".to_owned(),
            value: CACHE_CONTROL.to_owned(),
        });
    }
    Ok(ResponseParts {
        status: 200,
        headers,
        body,
    })
}

fn encode_error(
    transport: SubscriptionTransport,
    response: &GraphQlResponse,
    maximum: usize,
) -> Result<Vec<u8>, SubscriptionControlEncodingError> {
    let mut output = BoundedWriter::new(maximum);
    match transport {
        SubscriptionTransport::ServerSentEvents => {
            crate::subscription::event_encoding::write_http_error(&mut output, response)?;
        }
        SubscriptionTransport::Multipart => {
            super::frames::write_multipart_error(&mut output, &response.errors)?;
        }
        SubscriptionTransport::WebSocket => {
            return Err(SubscriptionControlEncodingError::InvalidTransportArguments);
        }
    }
    let complete = super::frames::static_bytes(transport, SubscriptionStaticControl::Complete)
        .ok_or(SubscriptionControlEncodingError::InvalidTransportArguments)?;
    output.bytes(complete)?;
    Ok(output.finish())
}

fn header_charge(name: &str, value: &str) -> Result<usize, JsonConversionError> {
    add(add(HEADER_ENTRY_BYTES, name.len())?, value.len())
}

fn add(left: usize, right: usize) -> Result<usize, JsonConversionError> {
    left.checked_add(right)
        .ok_or(JsonConversionError::ResponseSizeLimit)
}
