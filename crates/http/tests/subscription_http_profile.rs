#[path = "subscription_http_profile/invalid.rs"]
mod invalid;

use chigiri_http::{
    __negotiate_subscription_http_profile, RequestHeader, RequestMethod, RequestParts,
    ResponseMediaType, SubscriptionTransport, decode_request, negotiate_response_media,
};

fn select(accept: &[u8]) -> SubscriptionTransport {
    __negotiate_subscription_http_profile(
        "POST",
        &[
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("accept", accept),
        ],
    )
    .unwrap()
}

#[test]
fn sse_accepts_exact_and_wildcard_ranges_for_a_selected_subscription() {
    for accept in [
        b"text/event-stream".as_slice(),
        b"TEXT/EVENT-STREAM; charset=\"UTF-8\"",
        b" \ttext/event-stream \t; charset \t= utf-8 \t; q=0.001 \t",
        b"text/*",
        b"*/*",
        b", ,text/event-stream,,",
        b"application/json, text/event-stream",
        b"multipart/mixed;subscriptionSpec=0.9, text/event-stream",
        b"text/event-stream;charset=\"ut\\f-8\"",
    ] {
        assert_eq!(select(accept), SubscriptionTransport::ServerSentEvents);
    }
}

#[test]
fn multipart_requires_a_matching_profile_and_fixed_optional_boundary() {
    for accept in [
        b"multipart/mixed;subscriptionSpec=1.0".as_slice(),
        b"multipart/mixed; subscriptionSpec=\"1.0\"; boundary=\"graphql\"",
        b" \tMULTIPART/MIXED \t; SUBSCRIPTIONSPEC \t= \"1.0\" \t; q=1.000",
        b"multipart/mixed;boundary=graphql;subscriptionSpec=1.0",
        b"multipart/mixed;subscriptionSpec=\"1\\.0\";boundary=\"graph\\ql\"",
        b"multipart/*;subscriptionSpec=1.0",
        b"*/*;subscriptionSpec=1.0",
        b"multipart/mixed;subscriptionSpec=1.0;q=0.001",
    ] {
        assert_eq!(select(accept), SubscriptionTransport::Multipart);
    }
}

#[test]
fn quality_is_weight_at_every_parameter_position_without_affecting_precedence() {
    for accept in [
        b"text/event-stream;q=1;charset=utf-8".as_slice(),
        b"text/event-stream; Q=0.5; charset=\"UTF-8\"",
    ] {
        assert_eq!(select(accept), SubscriptionTransport::ServerSentEvents);
    }
    for accept in [
        b"multipart/mixed;q=0.5;subscriptionSpec=1.0;boundary=graphql".as_slice(),
        b"multipart/mixed; subscriptionSpec=\"1.0\"; Q=0.5; boundary=\"graphql\"",
        b"multipart/mixed;boundary=graphql;q=0.5;subscriptionSpec=1.0",
        b"multipart/mixed;boundary=graphql;subscriptionSpec=1.0;q=0.5",
        b"multipart/mixed;subscriptionSpec=1.0;boundary=graphql;q=0.5",
        b"multipart/mixed;q=0.5;boundary=graphql;subscriptionSpec=1.0",
        b"multipart/mixed;q=0.5;subscriptionSpec=1.0",
        b"text/event-stream;q=0.5, text/event-stream;q=0.1;charset=utf-8, multipart/mixed;q=0.3;subscriptionSpec=1.0",
        b"text/event-stream;q=0;charset=utf-8, */*;q=1, multipart/mixed;q=0.3;subscriptionSpec=1.0",
    ] {
        assert_eq!(select(accept), SubscriptionTransport::Multipart);
    }
}

#[test]
fn quality_is_selected_after_specificity_and_exact_zero_excludes_wildcards() {
    let cases: &[(&[u8], SubscriptionTransport)] = &[
        (
            b"text/event-stream;q=0, */*;q=1, multipart/mixed;subscriptionSpec=1.0;q=0.5",
            SubscriptionTransport::Multipart,
        ),
        (
            b"text/event-stream;q=0.5, text/event-stream;charset=utf-8;q=0.1, multipart/mixed;subscriptionSpec=1.0;q=0.3",
            SubscriptionTransport::Multipart,
        ),
        (
            b"text/*;q=0.8, text/event-stream;q=0.2, multipart/mixed;subscriptionSpec=1.0;q=0.3",
            SubscriptionTransport::Multipart,
        ),
        (
            b"multipart/mixed;subscriptionSpec=1.0;q=0.2, multipart/*;subscriptionSpec=1.0;q=0.9, text/event-stream;q=0.3",
            SubscriptionTransport::ServerSentEvents,
        ),
        (
            b"multipart/mixed;subscriptionSpec=1.0;q=0.5, text/event-stream;q=0.5",
            SubscriptionTransport::ServerSentEvents,
        ),
    ];
    for (accept, expected) in cases {
        assert_eq!(select(accept), *expected);
    }
}

#[test]
fn all_accept_lines_participate_in_selection() {
    let headers = [
        RequestHeader::new("CONTENT-TYPE", b"application/json; charset=\"UTF-8\""),
        RequestHeader::new("Accept", b"text/*;q=1"),
        RequestHeader::new("accept", b"text/event-stream;q=0.2"),
        RequestHeader::new("ACCEPT", b"multipart/mixed;subscriptionSpec=1.0;q=0.3"),
    ];
    assert_eq!(
        __negotiate_subscription_http_profile("POST", &headers).unwrap(),
        SubscriptionTransport::Multipart,
    );
    let reversed = headers.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(
        __negotiate_subscription_http_profile("POST", &reversed).unwrap(),
        SubscriptionTransport::Multipart,
    );
}

#[test]
fn quoted_delimiters_cannot_create_ranges_or_parameters() {
    assert_eq!(
        select(b"text/plain;unknown=\"a,b;c\\\"d\", text/event-stream"),
        SubscriptionTransport::ServerSentEvents,
    );
    assert_eq!(
        select(b"multipart/mixed;boundary=\"wrong,text/event-stream\";subscriptionSpec=1.0, text/event-stream;q=0.2"),
        SubscriptionTransport::ServerSentEvents,
    );
}

#[test]
fn repeated_ranges_use_the_highest_quality_at_equal_precedence() {
    for accept in [
        b"text/event-stream;q=0, text/event-stream;q=1".as_slice(),
        b"text/event-stream;q=1, text/event-stream;q=0",
    ] {
        assert_eq!(select(accept), SubscriptionTransport::ServerSentEvents);
    }
}

#[test]
fn valid_quality_boundaries_and_three_fractional_digits_are_exact() {
    for quality in ["1", "1.", "1.0", "1.00", "1.000", "0.001", "0.1", "0.01"] {
        let accept = format!("text/event-stream;q={quality}");
        assert_eq!(
            select(accept.as_bytes()),
            SubscriptionTransport::ServerSentEvents
        );
    }
}

#[test]
fn finite_codec_remains_finite_and_unchanged() {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"*/*"),
    ];
    assert_eq!(
        negotiate_response_media(&headers).unwrap(),
        ResponseMediaType::GraphQlResponseJson,
    );
    let decoded = decode_request(RequestParts {
        method: "POST",
        uri_query: None,
        headers: &headers,
        body: br#"{"query":"query { ordinary }"}"#,
    })
    .unwrap();
    assert_eq!(decoded.method, RequestMethod::Post);
    assert_eq!(
        decoded.response_media_type,
        ResponseMediaType::GraphQlResponseJson
    );
    let headers = [RequestHeader::new("accept", b"text/event-stream")];
    assert_eq!(
        negotiate_response_media(&headers).unwrap_err().status(),
        406
    );
}
