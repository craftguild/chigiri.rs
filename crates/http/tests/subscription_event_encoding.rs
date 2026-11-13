#[path = "subscription_event_encoding/support.rs"]
mod support;

use chigiri_core::{GraphQlError, GraphQlResponse, Location, Object, Value};
use chigiri_http::{
    __encode_subscription_event, JsonConversionError, SubscriptionEventEncodingError,
    SubscriptionTransport,
};

use support::{
    LARGE_LIMIT, assert_finalized_owner_contract, assert_in_order, delivery_counter, encode,
    executed_response, result_json,
};

const MAX_COUNTER: u64 = 9_007_199_254_740_991;

#[test]
fn finalization_uses_exact_fixed_width_counters() {
    let response = executed_response();
    let mut charged_length = None;

    for counter in [None, Some(0), Some(37), Some(MAX_COUNTER)] {
        let encoded = encode(SubscriptionTransport::ServerSentEvents, None, &response);
        assert_eq!(charged_length.get_or_insert(encoded.len()), &encoded.len());
        let bytes = encoded.finalize(counter).unwrap();
        assert_eq!(bytes.len(), *charged_length.as_ref().unwrap());
        assert_eq!(
            delivery_counter(&bytes, SubscriptionTransport::ServerSentEvents),
            counter
        );
    }

    assert_eq!(
        encode(SubscriptionTransport::ServerSentEvents, None, &response)
            .finalize(Some(MAX_COUNTER + 1))
            .unwrap_err(),
        SubscriptionEventEncodingError::DeliveryCounterOutOfRange
    );
}

#[test]
fn deferred_finalization_updates_a_simulated_queued_survivor() {
    let survivor = {
        let response = executed_response();
        encode(SubscriptionTransport::ServerSentEvents, None, &response)
    };
    let evicted = encode(
        SubscriptionTransport::ServerSentEvents,
        None,
        &executed_response(),
    );
    drop(evicted);

    let first_emission = survivor.finalize(Some(5)).unwrap();
    let later_emission = encode(
        SubscriptionTransport::ServerSentEvents,
        None,
        &executed_response(),
    )
    .finalize(Some(8))
    .unwrap();

    assert_eq!(
        delivery_counter(&first_emission, SubscriptionTransport::ServerSentEvents),
        Some(5)
    );
    assert_eq!(
        delivery_counter(&later_emission, SubscriptionTransport::ServerSentEvents),
        Some(8)
    );
    assert_eq!(
        delivery_counter(&first_emission, SubscriptionTransport::ServerSentEvents),
        Some(5)
    );
}

#[test]
fn metadata_merge_preserves_siblings_and_overwrites_the_reserved_leaf() {
    let mut delivery = Object::new();
    delivery.insert("before", true);
    delivery.insert("droppedTotal", "forged");
    delivery.insert("after", "kept");

    let mut chigiri = Object::new();
    chigiri.insert("left", 1_i32);
    chigiri.insert("delivery", delivery);
    chigiri.insert("right", 2_i32);

    let mut extensions = Object::new();
    extensions.insert("topBefore", "present");
    extensions.insert("chigiri", chigiri);
    extensions.insert("topAfter", "present");

    let mut response = executed_response();
    response.extensions = extensions;
    let bytes = encode(SubscriptionTransport::ServerSentEvents, None, &response)
        .finalize(Some(7))
        .unwrap();
    let text = std::str::from_utf8(bytes.as_bytes()).unwrap();
    let payload = result_json(&bytes, SubscriptionTransport::ServerSentEvents);

    assert_eq!(payload["extensions"]["topBefore"], "present");
    assert_eq!(payload["extensions"]["chigiri"]["left"], 1);
    assert_eq!(payload["extensions"]["chigiri"]["delivery"]["before"], true);
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["droppedTotal"],
        7
    );
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["after"],
        "kept"
    );
    assert_eq!(payload["extensions"]["chigiri"]["right"], 2);
    assert_eq!(payload["extensions"]["topAfter"], "present");
    assert_in_order(
        text,
        &[
            "topBefore",
            "chigiri",
            "left",
            "delivery",
            "before",
            "droppedTotal",
            "after",
            "right",
            "topAfter",
        ],
    );
}

#[test]
fn metadata_merge_creates_each_missing_reserved_object() {
    let response = executed_response();
    let bytes = encode(SubscriptionTransport::ServerSentEvents, None, &response)
        .finalize(None)
        .unwrap();
    assert_eq!(
        delivery_counter(&bytes, SubscriptionTransport::ServerSentEvents),
        None
    );

    let mut chigiri = Object::new();
    chigiri.insert("sibling", "kept");
    let mut response = executed_response();
    response.extensions.insert("chigiri", chigiri);
    let bytes = encode(SubscriptionTransport::ServerSentEvents, None, &response)
        .finalize(Some(0))
        .unwrap();
    let payload = result_json(&bytes, SubscriptionTransport::ServerSentEvents);
    assert_eq!(payload["extensions"]["chigiri"]["sibling"], "kept");
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["droppedTotal"],
        0
    );

    let mut delivery = Object::new();
    delivery.insert("sibling", "kept");
    let mut chigiri = Object::new();
    chigiri.insert("delivery", delivery);
    let mut response = executed_response();
    response.extensions.insert("chigiri", chigiri);
    let bytes = encode(SubscriptionTransport::ServerSentEvents, None, &response)
        .finalize(Some(2))
        .unwrap();
    let payload = result_json(&bytes, SubscriptionTransport::ServerSentEvents);
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["sibling"],
        "kept"
    );
    assert_eq!(
        payload["extensions"]["chigiri"]["delivery"]["droppedTotal"],
        2
    );
}

#[test]
fn non_object_reserved_prefixes_fail_without_exposing_values() {
    let mut chigiri_collision = executed_response();
    chigiri_collision
        .extensions
        .insert("chigiri", "private-chigiri-value");

    let mut chigiri = Object::new();
    chigiri.insert("delivery", "private-delivery-value");
    let mut delivery_collision = executed_response();
    delivery_collision.extensions.insert("chigiri", chigiri);

    for response in [&chigiri_collision, &delivery_collision] {
        let error = __encode_subscription_event(
            SubscriptionTransport::ServerSentEvents,
            None,
            response,
            LARGE_LIMIT,
        )
        .unwrap_err();
        assert_eq!(
            error,
            SubscriptionEventEncodingError::DeliveryExtensionCollision
        );
        assert!(!format!("{error:?} {error}").contains("private"));
    }
}

#[test]
fn every_profile_has_exact_framing_and_charges_all_of_it() {
    let response = executed_response();
    let slot = "0               ";
    let result = [
        r#"{"data":{"ok":true},"extensions":{"chigiri":{"delivery":{"droppedTotal":"#,
        slot,
        "}}}}",
    ]
    .concat();
    let cases = [
        (
            SubscriptionTransport::WebSocket,
            Some("op-1"),
            format!("{{\"id\":\"op-1\",\"type\":\"next\",\"payload\":{result}}}"),
        ),
        (
            SubscriptionTransport::ServerSentEvents,
            None,
            format!("event: next\ndata: {result}\n\n"),
        ),
        (
            SubscriptionTransport::Multipart,
            None,
            format!(
                "--graphql\r\nContent-Type: application/json\r\n\r\n{{\"payload\":{result}}}\r\n"
            ),
        ),
    ];

    for (transport, operation_id, expected) in cases {
        let bytes = __encode_subscription_event(transport, operation_id, &response, LARGE_LIMIT)
            .unwrap()
            .finalize(Some(0))
            .unwrap();
        assert_eq!(bytes.as_bytes(), expected.as_bytes(), "{transport:?}");

        let exact =
            __encode_subscription_event(transport, operation_id, &response, bytes.len()).unwrap();
        assert_eq!(exact.len(), bytes.len());
        assert_eq!(
            exact.finalize(Some(0)).unwrap().as_bytes(),
            bytes.as_bytes()
        );

        assert_eq!(
            __encode_subscription_event(transport, operation_id, &response, bytes.len() - 1)
                .unwrap_err(),
            SubscriptionEventEncodingError::Json(JsonConversionError::ResponseSizeLimit)
        );
    }
}

#[test]
fn application_error_locations_paths_and_extensions_use_the_shared_encoder() {
    let mut error = GraphQlError::new("bad field").code("BAD_FIELD");
    error.locations.push(Location::new(2, 9));
    error.path.push("viewer".into());
    error.path.push(3_usize.into());
    error.extensions.insert("detail", "safe");
    let response = GraphQlResponse::executed(Value::Null).with_error(error);

    for (transport, operation_id) in [
        (SubscriptionTransport::WebSocket, Some("operation")),
        (SubscriptionTransport::ServerSentEvents, None),
        (SubscriptionTransport::Multipart, None),
    ] {
        let bytes = encode(transport, operation_id, &response)
            .finalize(Some(0))
            .unwrap();
        let payload = result_json(&bytes, transport);
        assert_eq!(payload["errors"][0]["message"], "bad field");
        assert_eq!(payload["errors"][0]["locations"][0]["line"], 2);
        assert_eq!(payload["errors"][0]["locations"][0]["column"], 9);
        assert_eq!(
            payload["errors"][0]["path"],
            serde_json::json!(["viewer", 3])
        );
        assert_eq!(payload["errors"][0]["extensions"]["code"], "BAD_FIELD");
        assert_eq!(payload["errors"][0]["extensions"]["detail"], "safe");
    }
}

#[test]
fn profile_arguments_and_debug_output_are_safe() {
    let response = executed_response();
    for (transport, operation_id) in [
        (SubscriptionTransport::WebSocket, None),
        (SubscriptionTransport::ServerSentEvents, Some("private-id")),
        (SubscriptionTransport::Multipart, Some("private-id")),
    ] {
        let error = __encode_subscription_event(transport, operation_id, &response, LARGE_LIMIT)
            .unwrap_err();
        assert_eq!(
            error,
            SubscriptionEventEncodingError::InvalidTransportArguments
        );
        assert!(!format!("{error:?} {error}").contains("private-id"));
    }

    let encoded = encode(
        SubscriptionTransport::WebSocket,
        Some("private-operation-id"),
        &response,
    );
    let debug = format!("{encoded:?}");
    assert!(debug.contains("bytes"));
    assert!(!debug.contains("private-operation-id"));
}

#[test]
fn finalized_owner_is_move_only_and_exposes_only_immutable_bytes() {
    let finalized = encode(
        SubscriptionTransport::WebSocket,
        Some("private-operation-id"),
        &executed_response(),
    )
    .finalize(Some(4))
    .unwrap();

    let bytes: &[u8] = finalized.as_ref();
    assert_eq!(bytes, finalized.as_bytes());
    assert_eq!(bytes.len(), finalized.len());
    assert!(!finalized.is_empty());
    assert_finalized_owner_contract();

    let debug = format!("{finalized:?}");
    assert!(debug.contains("bytes"));
    assert!(!debug.contains("private-operation-id"));
}

#[test]
fn shared_json_failures_remain_bounded_safe_errors() {
    let response = GraphQlResponse::executed(Value::Float(f64::NAN));
    assert_eq!(
        __encode_subscription_event(
            SubscriptionTransport::ServerSentEvents,
            None,
            &response,
            LARGE_LIMIT,
        )
        .unwrap_err(),
        SubscriptionEventEncodingError::Json(JsonConversionError::NonFiniteFloat)
    );
}
