//! Conversion from transport-neutral response parts to a Hyper response.

use std::convert::Infallible;

use bytes::Bytes;
use chigiri_http::ResponseParts;
use http_body_util::Full;
use hyper::{Response, StatusCode};

pub(crate) fn into_response(parts: ResponseParts) -> Result<Response<Full<Bytes>>, Infallible> {
    let mut response = Response::new(Full::new(Bytes::from(parts.body)));
    *response.status_mut() =
        StatusCode::from_u16(parts.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for header in parts.headers {
        let Ok(name) = hyper::header::HeaderName::from_bytes(header.name.as_bytes()) else {
            continue;
        };
        let Ok(value) = hyper::header::HeaderValue::from_str(&header.value) else {
            continue;
        };
        response.headers_mut().append(name, value);
    }
    Ok(response)
}
