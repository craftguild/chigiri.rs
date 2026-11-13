//! Checked guide arithmetic and exact-fit production encoding, without a sizing API.

#[path = "subscription_sizing/payload.rs"]
mod payload;

use chigiri::SubscriptionLimits;
use chigiri_http::{
    __encode_subscription_event, JsonConversionError, SubscriptionEventEncodingError,
    SubscriptionTransport,
};

use super::support::resolvers;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
const MAX_COUNTER: u64 = 9_007_199_254_740_991;

fn plans() -> [(SubscriptionLimits, usize, usize); 2] {
    [
        (SubscriptionLimits::default(), 40 * MIB, 24 * MIB),
        (
            SubscriptionLimits {
                max_active_subscriptions: 8,
                max_queued_events: 4,
                max_queued_bytes: 2 * MIB,
                max_encoded_event_bytes: 768 * KIB,
                max_endpoint_encoded_bytes: 32 * MIB,
                ..SubscriptionLimits::default()
            },
            22 * MIB,
            10 * MIB,
        ),
    ]
}

fn allowance(active: usize, queue: usize, event: usize) -> Option<usize> {
    active.checked_mul(queue.checked_add(event)?)
}

#[test]
fn documented_default_and_large_plans_have_checked_headroom_and_valid_graphs() {
    let default = SubscriptionLimits::default();
    assert_eq!(default.max_active_subscriptions, 128);
    assert_eq!(default.max_queued_events, 32);
    assert_eq!(default.max_queued_bytes, 256 * KIB);
    assert_eq!(default.max_encoded_event_bytes, 64 * KIB);
    assert_eq!(default.max_endpoint_encoded_bytes, 64 * MIB);
    for (limits, expected_allowance, expected_headroom) in plans() {
        let storage = allowance(
            limits.max_active_subscriptions,
            limits.max_queued_bytes,
            limits.max_encoded_event_bytes,
        )
        .unwrap();
        assert_eq!(storage, expected_allowance);
        assert_eq!(
            limits.max_endpoint_encoded_bytes.checked_sub(storage),
            Some(expected_headroom)
        );
        let endpoint = chigiri::Endpoint::builder(resolvers())
            .subscription_limits(limits)
            .without_context()
            .build()
            .unwrap();
        assert_eq!(
            *endpoint.configuration().subscription_limits().effective(),
            limits
        );
    }
    assert_eq!(allowance(1, usize::MAX, 1), None);
    assert_eq!(allowance(usize::MAX, 1, 1), None);
    assert_eq!(allowance(2, usize::MAX / 2, 1), None);
}

#[test]
fn escaped_default_and_large_results_fit_exactly_in_every_production_profile() {
    let prefix = "op\"\\\n";
    let operation = format!("{prefix}{}", "x".repeat(256 - prefix.len()));
    for (limits, _, _) in plans() {
        let maximum = limits.max_encoded_event_bytes;
        for (profile, id) in [
            (SubscriptionTransport::WebSocket, Some(operation.as_str())),
            (SubscriptionTransport::ServerSentEvents, None),
            (SubscriptionTransport::Multipart, None),
        ] {
            let baseline = __encode_subscription_event(profile, id, &payload::response(0), maximum)
                .unwrap()
                .len();
            let padding = maximum.checked_sub(baseline).unwrap();
            let response = payload::response(padding);
            for counter in [None, Some(0), Some(MAX_COUNTER)] {
                let event = __encode_subscription_event(profile, id, &response, maximum)
                    .unwrap()
                    .finalize(counter)
                    .unwrap();
                assert_eq!(event.len(), maximum);
                payload::assert_result(event.as_bytes(), profile, padding, counter, id);
                assert_eq!(
                    __encode_subscription_event(profile, id, &response, maximum - 1).unwrap_err(),
                    SubscriptionEventEncodingError::Json(JsonConversionError::ResponseSizeLimit)
                );
            }
            assert_eq!(
                __encode_subscription_event(profile, id, &payload::response(padding + 1), maximum)
                    .unwrap_err(),
                SubscriptionEventEncodingError::Json(JsonConversionError::ResponseSizeLimit)
            );
            eprintln!(
                "guide event: profile={profile:?} limit={maximum} raw_message={} included_overhead={}",
                payload::PREFIX.len() + padding,
                baseline - payload::PREFIX.len()
            );
        }
    }
}
