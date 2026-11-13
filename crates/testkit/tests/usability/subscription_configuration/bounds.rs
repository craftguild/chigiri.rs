//! Public bounds and defaults are checked against normative literal fixtures.

use std::time::Duration;

use chigiri::{SubscriptionLimits as Limits, SubscriptionProtocolSettings as Protocol};

#[test]
fn all_public_bounds_match_the_adr_including_units() {
    let counts_and_bytes: [(usize, usize); 13] = [
        (Limits::MIN_COUNT, 1),
        (Limits::MIN_BYTES, 1),
        (Limits::MAX_ACTIVE_SUBSCRIPTIONS, 1_024),
        (Limits::MAX_QUEUED_EVENTS, 1_024),
        (Limits::MAX_QUEUED_BYTES, 4_194_304),
        (Limits::MAX_ENCODED_EVENT_BYTES, 1_048_576),
        (Limits::MAX_ENDPOINT_ENCODED_BYTES, 268_435_456),
        (Limits::MAX_PENDING_OPERATION_SETUPS, 128),
        (Limits::MAX_RETAINED_METADATA_BYTES, 65_536),
        (Protocol::MIN_COUNT, 1),
        (Protocol::MAX_ACTIVE_OPERATIONS_PER_CONNECTION, 128),
        (Protocol::MAX_WEBSOCKET_CONNECTIONS, 512),
        (Protocol::MAX_PENDING_SETUPS_PER_CONNECTION, 32),
    ];
    for (actual, expected) in counts_and_bytes {
        assert_eq!(actual, expected);
    }
    let durations: [(Duration, Duration); 7] = [
        (Limits::MIN_TIMEOUT, Duration::from_nanos(1)),
        (Limits::MAX_QUEUE_AGE, Duration::from_secs(60)),
        (
            Limits::MAX_SEND_NO_PROGRESS_TIMEOUT,
            Duration::from_secs(60),
        ),
        (Limits::MAX_SETUP_TIMEOUT, Duration::from_secs(60)),
        (Limits::MAX_EVENT_TIMEOUT, Duration::from_secs(120)),
        (Limits::MIN_HEARTBEAT_INTERVAL, Duration::from_secs(1)),
        (Limits::MAX_HEARTBEAT_INTERVAL, Duration::from_secs(60)),
    ];
    for (actual, expected) in durations {
        assert_eq!(actual, expected);
    }
}

#[test]
fn defaults_remain_exact_and_baseline_snapshots_are_immutable() {
    let limits = Limits {
        max_active_subscriptions: 128,
        max_queued_events: 32,
        max_queued_bytes: 262_144,
        max_encoded_event_bytes: 65_536,
        max_queue_age: Duration::from_secs(10),
        send_no_progress_timeout: Duration::from_secs(10),
        setup_timeout: Duration::from_secs(10),
        event_timeout: Duration::from_secs(30),
        max_endpoint_encoded_bytes: 67_108_864,
        max_pending_operation_setups: 16,
        max_retained_metadata_bytes: 16_384,
        heartbeat_interval: Duration::from_secs(15),
    };
    let protocol = Protocol {
        max_active_operations_per_connection: 16,
        max_websocket_connections: 64,
        max_pending_setups_per_connection: 8,
    };
    assert_eq!(Limits::default(), limits);
    assert_eq!(Protocol::default(), protocol);
    let endpoint = chigiri::Endpoint::builder(super::resolvers())
        .without_context()
        .build()
        .unwrap();
    let cloned = endpoint.clone();
    assert_eq!(endpoint.configuration(), cloned.configuration());
    let snapshot = cloned.configuration().subscription_limits();
    assert_eq!(*snapshot.baseline(), limits);
    assert_eq!(*snapshot.effective(), limits);
    assert_eq!(snapshot.origin(), chigiri::ConfigurationOrigin::Baseline);
    let snapshot = cloned.configuration().subscription_protocol();
    assert_eq!(*snapshot.baseline(), protocol);
    assert_eq!(*snapshot.effective(), protocol);
    assert_eq!(snapshot.origin(), chigiri::ConfigurationOrigin::Baseline);
}
