use std::sync::Arc;

use chigiri_http::{ResponseCompletionOutcome, ResponseHeader, ResponseMediaType, ResponseParts};

use super::{
    FAILED_BODY, HEADER_ENTRY_BYTES, InitialCompletionFallbackCharge, InitialCompletionFallbacks,
    InitialPreparationByteGuards, InitialResponseBufferGuard, MAX_INITIAL_RESPONSE_BYTES,
    TIMED_OUT_BODY, initial_response_charge, reserve_initial_completion_fallbacks,
    reserve_initial_preparation,
};
use crate::subscription::control::SubscriptionControlSignal;
use crate::subscription::delivery::EndpointByteOwner;

fn expected_fixed(media: ResponseMediaType, body: &[u8]) -> usize {
    body.len() + HEADER_ENTRY_BYTES + "content-type".len() + media.content_type().len()
}

#[test]
fn both_finite_media_have_prebuilt_distinct_once_only_callback_inputs() {
    for media in [
        ResponseMediaType::GraphQlResponseJson,
        ResponseMediaType::Json,
    ] {
        let expected = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
        let bytes = EndpointByteOwner::new(expected);
        let (mut inputs, charge) = reserve_initial_completion_fallbacks(&bytes, media).unwrap();
        assert_eq!(bytes.used(), expected);
        let failed = inputs.take(ResponseCompletionOutcome::Failed).unwrap();
        let timed_out = inputs.take(ResponseCompletionOutcome::TimedOut).unwrap();
        assert_eq!(failed.status, 500);
        assert_eq!(timed_out.status, 504);
        assert_eq!(failed.body, FAILED_BODY);
        assert_eq!(timed_out.body, TIMED_OUT_BODY);
        for response in [&failed, &timed_out] {
            assert_eq!(response.headers.len(), 1);
            assert_eq!(response.headers[0].name, "content-type");
            assert_eq!(response.headers[0].value, media.content_type());
        }
        assert!(matches!(
            inputs.take(ResponseCompletionOutcome::Failed),
            Err(SubscriptionControlSignal::Accounting)
        ));
        assert!(matches!(
            inputs.take(ResponseCompletionOutcome::TimedOut),
            Err(SubscriptionControlSignal::Accounting)
        ));
        drop((failed, timed_out, inputs));
        // The task owns values, not the independently retained physical charge.
        assert_eq!(bytes.used(), expected);
        drop(charge);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn fallback_exact_capacity_succeeds_and_one_byte_less_leaks_nothing() {
    let media = ResponseMediaType::GraphQlResponseJson;
    let expected = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
    let too_small = EndpointByteOwner::new(expected - 1);
    assert!(matches!(
        reserve_initial_completion_fallbacks(&too_small, media),
        Err(SubscriptionControlSignal::SharedByteCapacity)
    ));
    assert_eq!(too_small.used(), 0);
    assert!(!too_small.is_corrupted());
    let exact = EndpointByteOwner::new(expected);
    let (inputs, charge) = reserve_initial_completion_fallbacks(&exact, media).unwrap();
    assert_eq!(exact.used(), expected);
    drop((inputs, charge));
    assert_eq!(exact.used(), 0);
}

#[test]
fn invalid_outcome_does_not_consume_either_fallback_and_clear_is_idempotent() {
    let bytes = EndpointByteOwner::new(1024);
    let (mut inputs, charge) =
        reserve_initial_completion_fallbacks(&bytes, ResponseMediaType::Json).unwrap();
    for outcome in [
        ResponseCompletionOutcome::Encoded,
        ResponseCompletionOutcome::EncodingLimitExceeded,
    ] {
        assert!(matches!(
            inputs.take(outcome),
            Err(SubscriptionControlSignal::InvalidTransportArguments)
        ));
    }
    assert_eq!(
        inputs
            .take(ResponseCompletionOutcome::Failed)
            .unwrap()
            .status,
        500
    );
    assert_eq!(
        inputs
            .take(ResponseCompletionOutcome::TimedOut)
            .unwrap()
            .status,
        504
    );
    inputs.clear();
    inputs.clear();
    drop(charge);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn initial_response_bound_counts_body_header_strings_and_each_entry() {
    let mut response = ResponseParts {
        status: 200,
        headers: vec![ResponseHeader {
            name: "x".to_owned(),
            value: "v".to_owned(),
        }],
        body: vec![b'x'; MAX_INITIAL_RESPONSE_BYTES - HEADER_ENTRY_BYTES - 2],
    };
    assert_eq!(
        initial_response_charge(&response).unwrap(),
        MAX_INITIAL_RESPONSE_BYTES
    );
    response.body.push(b'x');
    assert_eq!(
        initial_response_charge(&response),
        Err(SubscriptionControlSignal::ControlLimit)
    );
    response.body.pop();
    response.headers.push(ResponseHeader {
        name: "y".to_owned(),
        value: String::new(),
    });
    assert_eq!(
        initial_response_charge(&response),
        Err(SubscriptionControlSignal::ControlLimit)
    );
}

#[test]
fn maximum_result_reservation_stays_charged_until_exact_handoff_last_drop() {
    let media = ResponseMediaType::Json;
    let fallback_bytes = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
    let bytes = EndpointByteOwner::new(MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    let (mut inputs, guards) = reserve_initial_preparation(&bytes, media).unwrap();
    assert_eq!(bytes.used(), MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    let response = inputs.take(ResponseCompletionOutcome::Failed).unwrap();
    inputs.clear();
    let actual = expected_fixed(media, FAILED_BODY);
    let (fallback_charge, buffer) = guards.into_parts();
    drop(fallback_charge);
    let physical = Arc::new(buffer.finish(&response).unwrap().into_in_flight());
    assert_eq!(bytes.used(), actual);
    let retained = Arc::clone(&physical);
    drop((physical, inputs, response));
    assert_eq!(bytes.used(), actual);
    drop(retained);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn failed_combined_admission_and_oversized_result_release_all_owned_charges() {
    let media = ResponseMediaType::Json;
    let fallback_bytes = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
    let insufficient = EndpointByteOwner::new(MAX_INITIAL_RESPONSE_BYTES + fallback_bytes - 1);
    assert!(matches!(
        reserve_initial_preparation(&insufficient, media),
        Err(SubscriptionControlSignal::SharedByteCapacity)
    ));
    assert_eq!(insufficient.used(), 0);
    let bytes = EndpointByteOwner::new(MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    let (inputs, guards) = reserve_initial_preparation(&bytes, media).unwrap();
    let (fallback_charge, buffer) = guards.into_parts();
    let response = ResponseParts {
        status: 200,
        headers: Vec::new(),
        body: vec![0; MAX_INITIAL_RESPONSE_BYTES + 1],
    };
    assert!(matches!(
        buffer.finish(&response),
        Err(SubscriptionControlSignal::ControlLimit)
    ));
    drop((inputs, response, fallback_charge));
    assert_eq!(bytes.used(), 0);
}

#[test]
fn split_preserves_quarantined_fallback_charge_after_wire_response_release() {
    let media = ResponseMediaType::GraphQlResponseJson;
    let fallback_bytes = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
    let bytes = EndpointByteOwner::new(MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    let (mut task_inputs, guards) = reserve_initial_preparation(&bytes, media).unwrap();
    let (fallback_charge, wire_buffer) = guards.into_parts();
    // The bridge owns a separate wire response; neither task input is consumed.
    let wire_response = ResponseParts {
        status: 504,
        headers: vec![ResponseHeader {
            name: "content-type".to_owned(),
            value: media.content_type().to_owned(),
        }],
        body: TIMED_OUT_BODY.to_vec(),
    };
    let wire_charge = wire_buffer.finish(&wire_response).unwrap();
    assert_eq!(
        bytes.used(),
        fallback_bytes + expected_fixed(media, TIMED_OUT_BODY)
    );
    drop((wire_response, wire_charge));
    assert_eq!(bytes.used(), fallback_bytes);
    let callback_input = task_inputs
        .take(ResponseCompletionOutcome::TimedOut)
        .unwrap();
    assert_eq!(callback_input.status, 504);
    drop((callback_input, task_inputs));
    // Only the actual worker join may dispose this independently retained guard.
    assert_eq!(bytes.used(), fallback_bytes);
    drop(fallback_charge);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn split_guards_can_transfer_together_without_releasing_either_reservation() {
    let media = ResponseMediaType::Json;
    let fallback_bytes = expected_fixed(media, FAILED_BODY) + expected_fixed(media, TIMED_OUT_BODY);
    let bytes = EndpointByteOwner::new(MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    let (task_inputs, guards) = reserve_initial_preparation(&bytes, media).unwrap();
    let retained_cleanup = guards.into_parts();
    drop(task_inputs);
    assert_eq!(bytes.used(), MAX_INITIAL_RESPONSE_BYTES + fallback_bytes);
    drop(retained_cleanup);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn fallback_values_and_guards_are_send_sync_static_and_not_clone() {
    fn assert_owner<T: Send + Sync + 'static>() {}
    assert_owner::<InitialCompletionFallbacks>();
    assert_owner::<InitialCompletionFallbackCharge>();
    assert_owner::<InitialPreparationByteGuards>();
    assert_owner::<InitialResponseBufferGuard>();
    let _ = <InitialCompletionFallbacks as AmbiguousIfClone<_>>::marker;
    let _ = <InitialCompletionFallbackCharge as AmbiguousIfClone<_>>::marker;
    let _ = <InitialPreparationByteGuards as AmbiguousIfClone<_>>::marker;
    let _ = <InitialResponseBufferGuard as AmbiguousIfClone<_>>::marker;
}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}
impl<T: ?Sized> AmbiguousIfClone<()> for T {}
struct Invalid;
impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
