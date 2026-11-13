use chigiri_http::{
    __decode_streaming_envelope, __negotiate_subscription_http_profile, __preflight_streaming_http,
    RequestHeader, RequestMethod, RequestParts, ResponseMediaType, SubscriptionTransport,
    decode_envelope,
};

fn json_headers(accept: &[u8]) -> [RequestHeader<'_>; 2] {
    [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept),
    ]
}

#[test]
fn stream_only_accept_survives_until_the_selected_kind_chooses_a_candidate() {
    let headers = json_headers(b"text/event-stream");
    let preflight = __preflight_streaming_http("POST", &headers).unwrap();
    assert_eq!(preflight.method(), RequestMethod::Post);
    assert!(preflight.body_required());
    assert_eq!(
        preflight.provisional_media(),
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(preflight.finite().unwrap_err().status(), 406);
    assert_eq!(
        preflight.streaming().unwrap(),
        SubscriptionTransport::ServerSentEvents
    );

    let decoded = __decode_streaming_envelope(
        &preflight,
        None,
        br#"{"query":"subscription Events { event }"}"#,
    )
    .unwrap();
    assert_eq!(
        decoded.envelope.document_text(),
        Some("subscription Events { event }")
    );
}

#[test]
fn query_or_mutation_with_stream_only_accept_has_a_finite_406_candidate() {
    for source in ["query Q { value }", "mutation M { update }"] {
        let headers = json_headers(b"text/event-stream");
        let preflight = __preflight_streaming_http("POST", &headers).unwrap();
        let decoded = __decode_streaming_envelope(
            &preflight,
            None,
            format!(r#"{{"query":"{source}"}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(decoded.envelope.document_text(), Some(source));
        assert_eq!(preflight.finite().unwrap_err().status(), 406);
    }
}

#[test]
fn get_keeps_finite_decoding_but_cannot_become_a_subscription_stream() {
    for accept in [
        b"text/event-stream".as_slice(),
        b"multipart/mixed;subscriptionSpec=1.0",
    ] {
        let headers = [RequestHeader::new("accept", accept)];
        let preflight = __preflight_streaming_http("GET", &headers).unwrap();
        assert_eq!(preflight.method(), RequestMethod::Get);
        assert!(!preflight.body_required());
        let response = preflight.streaming().unwrap_err().into_response_parts();
        assert_eq!(response.status, 405);
        assert!(
            response
                .headers
                .iter()
                .any(|header| header.name == "allow" && header.value == "POST")
        );
    }

    let headers = [RequestHeader::new("accept", b"text/event-stream")];
    let preflight = __preflight_streaming_http("GET", &headers).unwrap();
    let decoded = __decode_streaming_envelope(
        &preflight,
        Some("query=query+Q%7Bvalue%7D&operationName=Q"),
        b"ignored",
    )
    .unwrap();
    assert_eq!(decoded.envelope.operation_name(), Some("Q"));
}

#[test]
fn get_streaming_candidate_preserves_accept_first_error_precedence() {
    let cases: &[&[RequestHeader<'_>]] = &[
        &[],
        &[RequestHeader::new("accept", b"application/json")],
        &[RequestHeader::new("accept", b"text/event-stream;q=bogus")],
        &[RequestHeader::new("accept", b"text/event-stream;q=0")],
    ];
    for headers in cases {
        let expected = __negotiate_subscription_http_profile("GET", headers)
            .unwrap_err()
            .into_response_parts();
        let actual = __preflight_streaming_http("GET", headers)
            .unwrap()
            .streaming()
            .unwrap_err()
            .into_response_parts();
        assert_eq!(actual, expected);
        assert_eq!(actual.status, 406);
        assert!(!actual.headers.iter().any(|header| header.name == "allow"));
    }
}

#[test]
fn absent_mixed_wildcard_and_zero_quality_accept_are_independent_candidates() {
    let absent = [RequestHeader::new("content-type", b"application/json")];
    let preflight = __preflight_streaming_http("POST", &absent).unwrap();
    assert_eq!(
        preflight.finite().unwrap(),
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(preflight.streaming().unwrap_err().status(), 406);

    let wildcard = json_headers(b"*/*");
    let preflight = __preflight_streaming_http("POST", &wildcard).unwrap();
    assert_eq!(
        preflight.finite().unwrap(),
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(
        preflight.streaming().unwrap(),
        SubscriptionTransport::ServerSentEvents
    );

    let mixed = json_headers(
        b"application/graphql-response+json, text/event-stream;q=0, multipart/mixed;subscriptionSpec=1.0",
    );
    let preflight = __preflight_streaming_http("POST", &mixed).unwrap();
    assert_eq!(
        preflight.finite().unwrap(),
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(
        preflight.streaming().unwrap(),
        SubscriptionTransport::Multipart
    );
}

#[test]
fn method_and_content_type_are_final_before_accept_selection() {
    let headers = json_headers(b"text/event-stream");
    let response = __preflight_streaming_http("PUT", &headers)
        .unwrap_err()
        .into_response_parts();
    assert_eq!(response.status, 405);
    assert!(
        response
            .headers
            .iter()
            .any(|header| header.name == "allow" && header.value == "GET, POST")
    );

    for headers in [
        vec![
            RequestHeader::new("content-type", b"text/plain"),
            RequestHeader::new("accept", b"text/event-stream"),
        ],
        vec![RequestHeader::new("accept", b"text/plain")],
    ] {
        let error = __preflight_streaming_http("POST", &headers).unwrap_err();
        assert_eq!(error.status(), 415);
        assert_eq!(error.media_type(), ResponseMediaType::GraphQlResponseJson);
    }
}

#[test]
fn streaming_candidate_retains_its_stricter_json_content_type_profile() {
    let headers = [
        RequestHeader::new(
            "content-type",
            b"application/json;charset=utf-8;charset=utf-8",
        ),
        RequestHeader::new(
            "accept",
            b"application/graphql-response+json, text/event-stream",
        ),
    ];
    let preflight = __preflight_streaming_http("POST", &headers).unwrap();
    assert_eq!(
        preflight.finite().unwrap(),
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(preflight.streaming().unwrap_err().status(), 415);
}

#[test]
fn deferred_json_and_persisted_envelopes_match_the_finite_normalizer() {
    let headers = json_headers(b"application/graphql-response+json, text/event-stream");
    let bodies: &[&[u8]] = &[
        br#"{"query":"query Q($id:ID!){node(id:$id){id}}","operationName":"Q","variables":{"id":"n1"},"extensions":{"trace":true}}"#,
        br#"{"operationName":"Stored","variables":{"id":7},"extensions":{"persistedQuery":{"version":1,"sha256Hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}}"#,
    ];
    let preflight = __preflight_streaming_http("POST", &headers).unwrap();
    for body in bodies {
        let deferred = __decode_streaming_envelope(&preflight, None, body).unwrap();
        let finite = decode_envelope(RequestParts {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body,
        })
        .unwrap();
        assert_eq!(deferred, finite);
    }
}

#[test]
fn preflight_debug_does_not_retain_or_reveal_header_values() {
    let preflight = {
        let headers = [
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("accept", b"text/event-stream"),
            RequestHeader::new("authorization", b"Bearer super-secret"),
        ];
        __preflight_streaming_http("POST", &headers).unwrap()
    };
    let debug = format!("{preflight:?}");
    assert!(!debug.contains("super-secret"));
    assert_eq!(
        preflight.streaming().unwrap(),
        SubscriptionTransport::ServerSentEvents
    );
}
