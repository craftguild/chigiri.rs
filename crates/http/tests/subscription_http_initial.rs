use chigiri_core::{GraphQlError, GraphQlResponse, Location};
use chigiri_http::{
    __encode_subscription_http_initial, JsonConversionError, ResponseHeaders, ResponseParts,
    SubscriptionControlEncodingError, SubscriptionEventEncodingError, SubscriptionTransport,
};

const MAXIMUM: usize = 64 * 1024;
const SSE_CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";
const MULTIPART_CONTENT_TYPE: &str = "multipart/mixed; boundary=graphql; subscriptionSpec=1.0";
const CACHE_CONTROL: &str = "no-cache, no-transform";
const SSE_ERROR: &str = concat!(
    "event: next\ndata: {\"errors\":[{\"message\":\"invalid selection\",",
    "\"locations\":[{\"line\":2,\"column\":4}],\"path\":[\"chosen\"],",
    "\"extensions\":{\"code\":\"BAD_USER_INPUT\"}}],",
    "\"extensions\":{\"chigiri\":{\"delivery\":{\"droppedTotal\":null            }}}}",
    "\n\nevent: complete\ndata: \n\n"
);
const MULTIPART_ERROR: &str = concat!(
    "--graphql\r\nContent-Type: application/json\r\n\r\n",
    "{\"payload\":null,\"errors\":[{\"message\":\"invalid selection\",",
    "\"extensions\":{\"code\":\"BAD_USER_INPUT\"}}]}\r\n--graphql--"
);

#[test]
fn live_source_initial_response_has_only_profile_headers_and_no_prefetch_body() {
    for transport in http_profiles() {
        let response = encode(transport, None, usize::MAX).unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        assert_eq!(response.body.capacity(), 0);
        assert_eq!(response.headers[0].name, "content-type");
        assert_eq!(response.headers[0].value, content_type(transport));
        if transport == SubscriptionTransport::ServerSentEvents {
            assert_eq!(response.headers.len(), 2);
            assert_eq!(response.headers[1].name, "cache-control");
            assert_eq!(response.headers[1].value, CACHE_CONTROL);
        } else {
            assert_eq!(response.headers.len(), 1);
        }
        assert!(
            !response.headers.iter().any(|header| matches!(
                header.name.as_str(),
                "content-length" | "transfer-encoding"
            ))
        );
    }
}

#[test]
fn error_and_terminal_frames_match_explicit_profile_fixtures() {
    let error = request_error();
    let sse = encode(
        SubscriptionTransport::ServerSentEvents,
        Some(&error),
        MAXIMUM,
    )
    .unwrap();
    assert_eq!(sse.status, 200);
    assert_eq!(sse.body, SSE_ERROR.as_bytes());
    let multipart = encode(SubscriptionTransport::Multipart, Some(&error), MAXIMUM).unwrap();
    assert_eq!(multipart.status, 200);
    assert_eq!(multipart.body, MULTIPART_ERROR.as_bytes());
    // Projection changes only transport encoding, not the ordinary hook input.
    assert_eq!(error.errors[0].locations, [Location::new(2, 4)]);
    assert_eq!(error.errors[0].path, ["chosen".into()]);
}

#[test]
fn exact_limit_includes_headers_error_framing_and_final_completion() {
    let error = request_error();
    for (transport, expected_body) in [
        (SubscriptionTransport::ServerSentEvents, SSE_ERROR),
        (SubscriptionTransport::Multipart, MULTIPART_ERROR),
    ] {
        let exact = profile_header_charge(transport) + expected_body.len();
        let response = encode(transport, Some(&error), exact).unwrap();
        assert_eq!(response.body, expected_body.as_bytes());
        assert_eq!(charge(&response), exact);
        assert_size_error(encode(transport, Some(&error), exact - 1).unwrap_err());
        let completion_bytes = match transport {
            SubscriptionTransport::ServerSentEvents => b"event: complete\ndata: \n\n".len(),
            SubscriptionTransport::Multipart => b"--graphql--".len(),
            SubscriptionTransport::WebSocket => unreachable!(),
        };
        assert_size_error(encode(transport, Some(&error), exact - completion_bytes).unwrap_err());
    }
}

#[test]
fn hook_header_utf8_bytes_and_each_entry_share_the_same_hard_ceiling() {
    for transport in http_profiles() {
        let value_bytes = MAXIMUM - profile_header_charge(transport) - 16 - "x-note".len();
        let value = "a".repeat(value_bytes);
        let mut headers = ResponseHeaders::new();
        headers.append("X-Note", &value).unwrap();
        let response =
            __encode_subscription_http_initial(transport, None, headers, usize::MAX).unwrap();
        assert_eq!(charge(&response), MAXIMUM);
        assert_eq!(response.headers[0].name, "x-note");
        assert_eq!(response.headers[0].value, value);

        let mut headers = ResponseHeaders::new();
        headers.append("x-note", &(value + "x")).unwrap();
        assert_size_error(
            __encode_subscription_http_initial(transport, None, headers, usize::MAX).unwrap_err(),
        );

        let mut headers = ResponseHeaders::new();
        headers.append("x-note", "\u{e9}").unwrap();
        headers.append("x-note", "").unwrap();
        let exact = profile_header_charge(transport) + 2 * (16 + "x-note".len()) + 2;
        let response = __encode_subscription_http_initial(transport, None, headers, exact).unwrap();
        assert_eq!(charge(&response), exact);
        let expected_headers = if transport == SubscriptionTransport::ServerSentEvents {
            4
        } else {
            3
        };
        assert_eq!(response.headers.len(), expected_headers);
    }
}

#[test]
fn intrinsic_ceiling_rejects_large_application_error_even_with_unlimited_caller_input() {
    let error = GraphQlResponse::request_error(GraphQlError::new("x".repeat(MAXIMUM)));
    for transport in http_profiles() {
        assert_size_error(encode(transport, Some(&error), usize::MAX).unwrap_err());
        assert_size_error(encode(transport, None, 0).unwrap_err());
    }
}

#[test]
fn unsupported_websocket_and_non_error_initial_results_fail_closed() {
    assert_eq!(
        encode(SubscriptionTransport::WebSocket, None, MAXIMUM).unwrap_err(),
        SubscriptionControlEncodingError::InvalidTransportArguments
    );
    let empty = GraphQlResponse::executed(chigiri_core::Value::Null);
    for transport in http_profiles() {
        assert_eq!(
            encode(transport, Some(&empty), MAXIMUM).unwrap_err(),
            SubscriptionControlEncodingError::MissingErrors
        );
    }
}

#[test]
fn shared_sse_extension_validation_is_not_bypassed_by_initial_framing() {
    let mut error = request_error();
    error.extensions.insert("chigiri", true);
    assert_eq!(
        encode(
            SubscriptionTransport::ServerSentEvents,
            Some(&error),
            MAXIMUM
        )
        .unwrap_err(),
        SubscriptionControlEncodingError::Event(
            SubscriptionEventEncodingError::DeliveryExtensionCollision
        )
    );
}

fn http_profiles() -> [SubscriptionTransport; 2] {
    [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ]
}

fn content_type(transport: SubscriptionTransport) -> &'static str {
    match transport {
        SubscriptionTransport::ServerSentEvents => SSE_CONTENT_TYPE,
        SubscriptionTransport::Multipart => MULTIPART_CONTENT_TYPE,
        SubscriptionTransport::WebSocket => unreachable!(),
    }
}

fn profile_header_charge(transport: SubscriptionTransport) -> usize {
    let content_type = 16 + "content-type".len() + content_type(transport).len();
    if transport == SubscriptionTransport::ServerSentEvents {
        content_type + 16 + "cache-control".len() + CACHE_CONTROL.len()
    } else {
        content_type
    }
}

fn request_error() -> GraphQlResponse {
    let mut error = GraphQlError::new("invalid selection").code("BAD_USER_INPUT");
    error.locations.push(Location::new(2, 4));
    error.path.push("chosen".into());
    GraphQlResponse::request_error(error)
}

fn encode(
    transport: SubscriptionTransport,
    response: Option<&GraphQlResponse>,
    limit: usize,
) -> Result<ResponseParts, SubscriptionControlEncodingError> {
    __encode_subscription_http_initial(transport, response, ResponseHeaders::new(), limit)
}

fn charge(response: &ResponseParts) -> usize {
    response.body.len()
        + response
            .headers
            .iter()
            .map(|header| 16 + header.name.len() + header.value.len())
            .sum::<usize>()
}

fn assert_size_error(error: SubscriptionControlEncodingError) {
    assert!(matches!(
        error,
        SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit)
            | SubscriptionControlEncodingError::Event(SubscriptionEventEncodingError::Json(
                JsonConversionError::ResponseSizeLimit
            ))
    ));
}
