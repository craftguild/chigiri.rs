//! Axum response construction for prepared and supervised streams.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::Response;
use chigiri::__streaming::{SubscriptionInitialResponse, SupervisedHttpResponse};
use chigiri_http::ResponseHeader;

use super::body::{AxumStreamingBody, prepared_finite_body, prepared_streaming_body};

pub(crate) fn prepared(response: SubscriptionInitialResponse) -> Response {
    let streaming = response.is_streaming_profile();
    let mut output = Response::new(Body::empty());
    apply_head(
        &mut output,
        response.status(),
        response.headers(),
        streaming,
    );
    *output.body_mut() = if streaming {
        prepared_streaming_body(response)
    } else {
        prepared_finite_body(response)
    };
    output
}

pub(crate) fn streaming(response: SupervisedHttpResponse) -> Response {
    let mut output = Response::new(Body::empty());
    apply_head(&mut output, response.status(), response.headers(), true);
    *output.body_mut() = Body::from_stream(AxumStreamingBody::new(response.into_body()));
    output
}

fn apply_head(response: &mut Response, status: u16, headers: &[ResponseHeader], streaming: bool) {
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for value in headers {
        let Ok(name) = HeaderName::from_bytes(value.name.as_bytes()) else {
            continue;
        };
        if streaming && name == header::CONTENT_LENGTH {
            continue;
        }
        let Ok(value) = HeaderValue::from_str(&value.value) else {
            continue;
        };
        response.headers_mut().append(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_head_rejects_content_length_and_invalid_values() {
        let mut response = Response::new(Body::empty());
        let headers = [
            header("content-type", "text/event-stream"),
            header("content-length", "999"),
            header("bad header", "ignored"),
        ];
        apply_head(&mut response, 200, &headers, true);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        assert_eq!(response.headers().len(), 1);
    }

    #[test]
    fn prepared_head_preserves_exact_length_and_fails_closed_on_bad_status() {
        let mut response = Response::new(Body::empty());
        let headers = [header("content-length", "4")];
        apply_head(&mut response, 99, &headers, false);

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "4");
    }

    fn header(name: &str, value: &str) -> ResponseHeader {
        ResponseHeader {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }
}
