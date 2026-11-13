use chigiri_http::{__negotiate_subscription_http_profile, RequestHeader};

#[test]
fn unsupported_or_malformed_ranges_do_not_admit_a_stream() {
    for accept in [
        b"".as_slice(),
        b"application/json",
        b"text/event-stream;q=0",
        b"text/event-stream;q=0.000, */*;q=1",
        b"text/event-stream;charset=latin1",
        b"text/event-stream;unknown=value",
        b"text/event-stream; charset=utf-8; charset=utf-8",
        b"text/event-stream; q=1; q=1",
        b"text/event-stream; q=1; charset=utf-8; Q=1",
        b"text/event-stream; q=1; charset=latin1",
        b"text/event-stream; q=1; charset=\"utf-8",
        b"multipart/mixed; q=1; subscriptionSpec=1.0; q=1",
        b"multipart/mixed; q=1; subscriptionSpec=1.0; boundary=wrong",
        b"text/event-stream;q=\"1\"",
        b"text/event-stream;q=01",
        b"text/event-stream;q=.5",
        b"text/event-stream;q=0.0001",
        b"text/event-stream;q=1.001",
        b"text/event-stream;q=-1",
        b"text/event-stream;q=2",
        b"text/event-stream; charset=\"utf-8",
        b"text/event-stream; charset=\"utf-8\\\"",
        b"text/event-stream; charset=\"utf-8\"extra",
        b"text/event-stream; charset=\"utf-8\"\"",
        b"text/event-stream; charset=\"utf\r-8\"",
        b"text/event-stream; charset=\"utf\n-8\"",
        b"text/event-stream; charset=\"utf\x7f-8\"",
        b"\ntext/event-stream",
        b"*/event-stream",
        b"text /event-stream",
        b"text/event-stream/other",
        b"text/event-stream;",
        b"multipart/mixed",
        b"multipart/*",
        b"multipart/mixed;subscriptionSpec=0.9",
        b"multipart/mixed;subscriptionSpec=1.0;boundary=wrong",
        b"multipart/mixed;subscriptionSpec=1.0;boundary=GraphQL",
        b"multipart/mixed;subscriptionSpec=1.0;charset=utf-8",
        b"multipart/mixed;subscriptionSpec=1.0;subscriptionSpec=1.0",
        b"multipart/mixed;subscriptionSpec=1.0;q=0",
        b"multipart/mixed;subscriptionSpec=1.0;q=0, multipart/*;subscriptionSpec=1.0;q=1",
        b"multipart/mixed;q=0, multipart/*;subscriptionSpec=1.0;q=1",
        b"text/event-stream;q=0, */*;q=1, multipart/mixed;subscriptionSpec=1.0;q=.5",
        b"text/event-stream;unknown=\"a,text/event-stream\"",
        b"\xfftext/event-stream",
    ] {
        let headers = [
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("accept", accept),
        ];
        assert_eq!(
            __negotiate_subscription_http_profile("POST", &headers)
                .unwrap_err()
                .status(),
            406,
            "unexpected acceptance for {accept:?}",
        );
    }
    assert_eq!(
        __negotiate_subscription_http_profile(
            "POST",
            &[RequestHeader::new("content-type", b"application/json")],
        )
        .unwrap_err()
        .status(),
        406,
    );
}

#[test]
fn unsupported_methods_are_rejected_with_post_only_allow() {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"text/event-stream"),
    ];
    for method in ["GET", "PUT", "DELETE", "OPTIONS", "post", "CONNECT"] {
        let error = __negotiate_subscription_http_profile(method, &headers).unwrap_err();
        assert_eq!(error.status(), 405);
        let response = error.into_response_parts();
        assert!(
            response
                .headers
                .iter()
                .any(|header| header.name == "allow" && header.value == "POST")
        );
    }
}

#[test]
fn content_type_is_one_json_value_with_optional_well_formed_utf8_charset() {
    for content_type in [
        b"text/plain".as_slice(),
        b"multipart/form-data;boundary=graphql",
        b"application/json;charset=latin1",
        b"application/json;unknown=value",
        b"application/json;charset=\"utf-8",
        b"application/json;charset=utf-8\"",
        b"application/json;charset=utf-8;charset=utf-8",
        b"application/json;q=1",
        b"application/json;",
        b"application/json, application/json",
        b"\xffapplication/json",
    ] {
        let headers = [
            RequestHeader::new("content-type", content_type),
            RequestHeader::new("accept", b"text/event-stream"),
        ];
        assert_eq!(
            __negotiate_subscription_http_profile("POST", &headers)
                .unwrap_err()
                .status(),
            415,
        );
    }
    let headers = [
        RequestHeader::new("accept", b"text/event-stream"),
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("Content-Type", b"application/json"),
    ];
    assert_eq!(
        __negotiate_subscription_http_profile("POST", &headers)
            .unwrap_err()
            .status(),
        415,
    );
    assert_eq!(
        __negotiate_subscription_http_profile(
            "POST",
            &[RequestHeader::new("accept", b"text/event-stream")],
        )
        .unwrap_err()
        .status(),
        415,
    );
}

#[test]
fn public_errors_do_not_echo_header_values() {
    let secret = b"application/json;token=PRIVATE_CREDENTIAL";
    let headers = [
        RequestHeader::new("accept", b"text/event-stream"),
        RequestHeader::new("content-type", secret),
    ];
    let error = __negotiate_subscription_http_profile("POST", &headers).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("PRIVATE_CREDENTIAL"));
    let body = error.into_response_parts().body;
    assert!(
        !std::str::from_utf8(&body)
            .unwrap()
            .contains("PRIVATE_CREDENTIAL")
    );
}
