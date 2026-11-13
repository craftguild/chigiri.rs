use chigiri_core::{GraphQlError, GraphQlResponse, Value};
use chigiri_http::{
    __encode_subscription_control, JsonConversionError, SubscriptionControl,
    SubscriptionControlEncodingError, SubscriptionEventEncodingError, SubscriptionTransport,
};

use super::support::{LARGE_LIMIT, assert_owner_contract, encode, terminal_response};

#[test]
fn invalid_profile_id_and_counter_combinations_fail_safely() {
    let response = terminal_response();
    let private_id = "private-operation-id";
    let cases = [
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionControl::ConnectionAck,
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionControl::Pong { payload: None },
        ),
        (
            SubscriptionTransport::WebSocket,
            SubscriptionControl::Complete {
                websocket_operation_id: None,
            },
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionControl::Complete {
                websocket_operation_id: Some(private_id),
            },
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionControl::Error {
                websocket_operation_id: Some(private_id),
                response: &response,
                dropped_total: None,
            },
        ),
        (
            SubscriptionTransport::WebSocket,
            SubscriptionControl::Error {
                websocket_operation_id: Some(private_id),
                response: &response,
                dropped_total: Some(1),
            },
        ),
    ];

    for (transport, control) in cases {
        let error = __encode_subscription_control(transport, control, LARGE_LIMIT).unwrap_err();
        assert_eq!(
            error,
            SubscriptionControlEncodingError::InvalidTransportArguments
        );
        assert!(!format!("{error:?} {error}").contains(private_id));
    }

    let too_long = "x".repeat(257);
    for operation_id in ["", too_long.as_str()] {
        let error = __encode_subscription_control(
            SubscriptionTransport::WebSocket,
            SubscriptionControl::Complete {
                websocket_operation_id: Some(operation_id),
            },
            LARGE_LIMIT,
        )
        .unwrap_err();
        assert_eq!(
            error,
            SubscriptionControlEncodingError::InvalidWebSocketOperationId
        );
        if !operation_id.is_empty() {
            assert!(!format!("{error:?} {error}").contains(operation_id));
        }
    }

    assert_eq!(
        __encode_subscription_control(
            SubscriptionTransport::WebSocket,
            SubscriptionControl::Error {
                websocket_operation_id: Some("operation"),
                response: &GraphQlResponse::executed(Value::Null),
                dropped_total: None,
            },
            LARGE_LIMIT,
        )
        .unwrap_err(),
        SubscriptionControlEncodingError::MissingErrors
    );
}

#[test]
fn hard_control_ceiling_rejects_oversized_messages_for_every_profile() {
    let response = GraphQlResponse::request_error(GraphQlError::new("x".repeat(70 * 1024)));
    let cases = [
        (
            SubscriptionTransport::WebSocket,
            SubscriptionControl::Error {
                websocket_operation_id: Some("operation"),
                response: &response,
                dropped_total: None,
            },
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            SubscriptionControl::Error {
                websocket_operation_id: None,
                response: &response,
                dropped_total: None,
            },
        ),
        (
            SubscriptionTransport::Multipart,
            SubscriptionControl::Error {
                websocket_operation_id: None,
                response: &response,
                dropped_total: None,
            },
        ),
    ];

    for (transport, control) in cases {
        let error = __encode_subscription_control(transport, control, usize::MAX).unwrap_err();
        assert!(matches!(
            error,
            SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit)
                | SubscriptionControlEncodingError::Event(SubscriptionEventEncodingError::Json(
                    JsonConversionError::ResponseSizeLimit
                ))
        ));
    }
}

#[test]
fn dynamic_entry_limits_include_every_transport_framing_byte() {
    let response = terminal_response();
    for transport in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        let encoded = encode_error(transport, &response, LARGE_LIMIT).unwrap();
        let exact = encode_error(transport, &response, encoded.len()).unwrap();
        assert_eq!(exact.as_bytes(), encoded.as_bytes());

        let error = encode_error(transport, &response, encoded.len() - 1).unwrap_err();
        assert!(matches!(
            error,
            SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit)
                | SubscriptionControlEncodingError::Event(SubscriptionEventEncodingError::Json(
                    JsonConversionError::ResponseSizeLimit
                ))
        ));
    }
}

#[test]
fn encoded_owner_is_move_only_and_debug_output_is_length_only() {
    let control = SubscriptionControl::Complete {
        websocket_operation_id: Some("private-operation-id"),
    };
    assert!(!format!("{control:?}").contains("private-operation-id"));
    let encoded = encode(SubscriptionTransport::WebSocket, control, LARGE_LIMIT);
    assert_eq!(encoded.as_ref(), encoded.as_bytes());
    assert_eq!(encoded.len(), encoded.as_bytes().len());
    assert!(!encoded.is_empty());
    assert_owner_contract();
    let debug = format!("{encoded:?}");
    assert!(debug.contains("bytes"));
    assert!(!debug.contains("private-operation-id"));
}

fn encode_error(
    transport: SubscriptionTransport,
    response: &GraphQlResponse,
    maximum_bytes: usize,
) -> Result<chigiri_http::EncodedSubscriptionControl, SubscriptionControlEncodingError> {
    __encode_subscription_control(
        transport,
        SubscriptionControl::Error {
            websocket_operation_id: (transport == SubscriptionTransport::WebSocket)
                .then_some("operation"),
            response,
            dropped_total: None,
        },
        maximum_bytes,
    )
}
