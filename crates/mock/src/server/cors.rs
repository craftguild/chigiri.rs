//! Enforce application-owned exact-origin CORS and GraphQL preflight responses.

use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(super) fn origin(
    request: &Request,
    origins: &[String],
) -> Result<Option<HeaderValue>, StatusCode> {
    let mut supplied = request.headers().get_all(header::ORIGIN).iter();
    let Some(origin) = supplied.next() else {
        return Ok(None);
    };
    if supplied.next().is_some()
        || !origins
            .iter()
            .any(|allowed| origin.as_bytes() == allowed.as_bytes())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(origin.clone()))
}

pub(super) fn preflight(request: &Request, origin: Option<HeaderValue>) -> Response {
    if origin.is_none()
        || !request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .is_some_and(|method| matches!(method.as_bytes(), b"GET" | b"POST"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .is_some_and(|headers| {
            headers.to_str().map_or(true, |headers| {
                headers.split(',').any(|header| {
                    !matches!(
                        header.trim().to_ascii_lowercase().as_str(),
                        "content-type" | "accept" | "authorization"
                    )
                })
            })
        })
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = cors(StatusCode::NO_CONTENT.into_response(), origin);
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Accept, Authorization"),
    );
    response
}

pub(super) fn cors(mut response: Response, origin: Option<HeaderValue>) -> Response {
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("Origin"));
    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response
}
