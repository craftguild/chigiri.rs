//! Conversion from transport-neutral response parts to an Axum response.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use chigiri_http::ResponseParts;

pub(crate) fn into_response(parts: ResponseParts) -> Response {
    let mut response = Response::new(Body::from(parts.body));
    *response.status_mut() =
        StatusCode::from_u16(parts.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for header in parts.headers {
        let Ok(name) = HeaderName::from_bytes(header.name.as_bytes()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_str(&header.value) else {
            continue;
        };
        response.headers_mut().append(name, value);
    }
    response
}
