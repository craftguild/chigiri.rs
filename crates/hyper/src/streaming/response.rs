//! Hyper response construction for finite, prepared, and supervised bodies.

use chigiri::__streaming::{SubscriptionInitialResponse, SupervisedHttpResponse};
use chigiri_http::{ResponseHeader, ResponseParts};
use hyper::{Response, StatusCode, header};

use super::HyperStreamingBody;

pub(crate) fn finite(parts: ResponseParts) -> Response<HyperStreamingBody> {
    let ResponseParts {
        status,
        headers,
        body,
    } = parts;
    build(
        status,
        &headers,
        HyperStreamingBody::finite(body),
        Length::Exact,
    )
}

pub(crate) fn prepared(response: SubscriptionInitialResponse) -> Response<HyperStreamingBody> {
    let length = if response.is_streaming_profile() {
        Length::Streaming
    } else {
        Length::Exact
    };
    let mut output = empty();
    apply_head(&mut output, response.status(), response.headers(), length);
    *output.body_mut() = HyperStreamingBody::prepared(response);
    output
}

pub(crate) fn streaming(response: SupervisedHttpResponse) -> Response<HyperStreamingBody> {
    let mut output = empty();
    apply_head(
        &mut output,
        response.status(),
        response.headers(),
        Length::Streaming,
    );
    *output.body_mut() = HyperStreamingBody::streaming(response.into_body());
    output
}

#[derive(Clone, Copy)]
enum Length {
    Exact,
    Streaming,
}

fn build(
    status: u16,
    headers: &[ResponseHeader],
    body: HyperStreamingBody,
    length: Length,
) -> Response<HyperStreamingBody> {
    let mut response = Response::new(body);
    apply_head(&mut response, status, headers, length);
    response
}

fn empty() -> Response<HyperStreamingBody> {
    Response::new(HyperStreamingBody::finite(Vec::new()))
}

fn apply_head(
    response: &mut Response<HyperStreamingBody>,
    status: u16,
    headers: &[ResponseHeader],
    length: Length,
) {
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for value in headers {
        let Ok(name) = header::HeaderName::from_bytes(value.name.as_bytes()) else {
            continue;
        };
        if matches!(length, Length::Streaming) && name == header::CONTENT_LENGTH {
            continue;
        }
        let Ok(value) = header::HeaderValue::from_str(&value.value) else {
            continue;
        };
        response.headers_mut().append(name, value);
    }
}

#[cfg(test)]
mod tests {
    use hyper::body::Body as _;

    use super::*;

    #[test]
    fn finite_response_preserves_head_and_exact_body_length() {
        let response = finite(ResponseParts {
            status: 202,
            headers: vec![ResponseHeader {
                name: "content-type".to_owned(),
                value: "application/graphql-response+json".to_owned(),
            }],
            body: b"finite".to_vec(),
        });

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/graphql-response+json"
        );
        assert_eq!(response.body().size_hint().exact(), Some(6));
    }

    #[test]
    fn invalid_finite_head_matches_the_existing_safe_adapter_behavior() {
        let response = finite(ResponseParts {
            status: 99,
            headers: vec![ResponseHeader {
                name: "not a header".to_owned(),
                value: "ignored".to_owned(),
            }],
            body: Vec::new(),
        });

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.headers().is_empty());
        assert_eq!(response.body().size_hint().exact(), Some(0));
    }

    #[test]
    fn streaming_head_rejects_content_length_and_invalid_values() {
        let mut response = empty();
        let headers = [
            ResponseHeader {
                name: "content-type".to_owned(),
                value: "text/event-stream".to_owned(),
            },
            ResponseHeader {
                name: "content-length".to_owned(),
                value: "999".to_owned(),
            },
            ResponseHeader {
                name: "bad header".to_owned(),
                value: "ignored".to_owned(),
            },
        ];
        apply_head(&mut response, 200, &headers, Length::Streaming);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        assert_eq!(response.headers().len(), 1);
    }
}
