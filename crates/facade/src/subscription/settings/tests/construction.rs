//! Default, maximum, and replacement construction tests.

use std::time::Duration;

use super::super::{ConnectionInitAuthorization, SubscriptionLimits, SubscriptionProtocolSettings};
use super::support::{assert_snapshot, build};
use crate::Endpoint;
use crate::configuration::ConfigurationOrigin;
use crate::subscription::request::test_support::schema;

#[test]
fn defaults_and_hard_maxima_construct_with_exact_snapshots() {
    let baseline = build(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    )
    .expect("accepted defaults must construct");
    assert_snapshot(
        baseline.configuration().subscription_limits(),
        SubscriptionLimits::default(),
        ConfigurationOrigin::User,
    );
    assert_snapshot(
        baseline.configuration().subscription_protocol(),
        SubscriptionProtocolSettings::default(),
        ConfigurationOrigin::User,
    );

    let maximum = SubscriptionLimits {
        max_active_subscriptions: 1_024,
        max_queued_events: 1_024,
        max_queued_bytes: 4 * 1024 * 1024,
        max_encoded_event_bytes: 1024 * 1024,
        max_queue_age: Duration::from_secs(60),
        send_no_progress_timeout: Duration::from_secs(60),
        setup_timeout: Duration::from_secs(60),
        event_timeout: Duration::from_secs(120),
        max_endpoint_encoded_bytes: 256 * 1024 * 1024,
        max_pending_operation_setups: 128,
        max_retained_metadata_bytes: 64 * 1024,
        heartbeat_interval: Duration::from_secs(60),
    };
    let protocol = SubscriptionProtocolSettings {
        max_active_operations_per_connection: 128,
        max_websocket_connections: 512,
        max_pending_setups_per_connection: 32,
    };
    let endpoint = build(maximum, protocol).expect("inclusive hard maxima must construct");
    assert_eq!(
        endpoint.configuration().subscription_limits().effective(),
        &maximum
    );
    assert_eq!(
        endpoint.configuration().subscription_protocol().effective(),
        &protocol
    );

    let heartbeat_minimum = SubscriptionLimits {
        heartbeat_interval: Duration::from_secs(1),
        ..SubscriptionLimits::default()
    };
    build(heartbeat_minimum, SubscriptionProtocolSettings::default())
        .expect("inclusive heartbeat minimum must construct");

    let positive_minimum = SubscriptionLimits {
        max_active_subscriptions: 1,
        max_queued_events: 1,
        max_queued_bytes: 1,
        max_encoded_event_bytes: 1,
        max_queue_age: Duration::from_nanos(1),
        send_no_progress_timeout: Duration::from_nanos(1),
        setup_timeout: Duration::from_nanos(1),
        event_timeout: Duration::from_nanos(1),
        max_endpoint_encoded_bytes: 1,
        max_pending_operation_setups: 1,
        max_retained_metadata_bytes: 1,
        heartbeat_interval: Duration::from_secs(1),
    };
    let protocol_minimum = SubscriptionProtocolSettings {
        max_active_operations_per_connection: 1,
        max_websocket_connections: 1,
        max_pending_setups_per_connection: 1,
    };
    build(positive_minimum, protocol_minimum)
        .expect("positive values below defaults must construct");
}

#[test]
fn untouched_defaults_retain_baseline_origin() {
    let endpoint = Endpoint::builder(schema())
        .without_context()
        .build()
        .unwrap();
    assert_snapshot(
        endpoint.configuration().subscription_limits(),
        SubscriptionLimits::default(),
        ConfigurationOrigin::Baseline,
    );
    assert_snapshot(
        endpoint.configuration().subscription_protocol(),
        SubscriptionProtocolSettings::default(),
        ConfigurationOrigin::Baseline,
    );
    assert_eq!(
        endpoint
            .configuration()
            .connection_init_authorization()
            .effective(),
        &ConnectionInitAuthorization::DefaultAccept
    );
}

#[test]
fn sugar_and_inject_share_slots_and_last_whole_value_wins() {
    let first = SubscriptionLimits {
        max_queued_events: 5,
        ..SubscriptionLimits::default()
    };
    let second = SubscriptionLimits {
        max_queued_events: 7,
        ..SubscriptionLimits::default()
    };
    let third = SubscriptionLimits {
        max_queued_events: 9,
        ..SubscriptionLimits::default()
    };
    let first_protocol = SubscriptionProtocolSettings {
        max_pending_setups_per_connection: 2,
        ..SubscriptionProtocolSettings::default()
    };
    let protocol = SubscriptionProtocolSettings {
        max_pending_setups_per_connection: 3,
        ..SubscriptionProtocolSettings::default()
    };
    let sugar = build(third, protocol).unwrap();
    let injected = Endpoint::builder(schema())
        .subscription_limits(first)
        .subscription_protocol(first_protocol)
        .inject(|components| {
            components.subscriptions().limits(second).protocol(protocol);
        })
        .without_context()
        .subscription_limits(third)
        .build()
        .unwrap();
    assert_eq!(
        sugar.configuration().subscription_limits(),
        injected.configuration().subscription_limits()
    );
    assert_eq!(
        sugar.configuration().subscription_protocol(),
        injected.configuration().subscription_protocol()
    );
    assert_eq!(
        injected.configuration().subscription_limits().effective(),
        &third
    );
}

#[test]
fn cloned_snapshots_remain_immutable_and_independent_builds_diverge() {
    let original = build(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    )
    .unwrap();
    let cloned = original.clone();
    let replacement = SubscriptionLimits {
        max_active_subscriptions: 7,
        ..SubscriptionLimits::default()
    };
    let independent = build(replacement, SubscriptionProtocolSettings::default()).unwrap();

    assert_eq!(original.configuration(), cloned.configuration());
    assert_eq!(
        cloned.configuration().subscription_limits().effective(),
        &SubscriptionLimits::default()
    );
    assert_eq!(
        independent
            .configuration()
            .subscription_limits()
            .effective(),
        &replacement
    );
}
