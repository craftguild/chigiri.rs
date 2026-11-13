//! Invalid boundary and cross-limit validation tests.

use std::time::Duration;

use super::super::{SubscriptionLimits, SubscriptionProtocolSettings};
use super::support::{build, diagnostic};
use crate::build_support::{ConfigurationSlot, ConfigurationViolation};
use crate::configuration::ConfigurationOrigin;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

#[test]
fn every_delivery_limit_rejects_zero_and_one_over_maximum() {
    use ConfigurationSlot as Slot;
    use ConfigurationViolation::{OutsideHardBound, ZeroDuration};

    invalid_limit(
        |value| value.max_active_subscriptions = 0,
        Slot::ActiveSubscriptions,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_active_subscriptions = 1_025,
        Slot::ActiveSubscriptions,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_queued_events = 0,
        Slot::QueuedSubscriptionEvents,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_queued_events = 1_025,
        Slot::QueuedSubscriptionEvents,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_queued_bytes = 0,
        Slot::QueuedSubscriptionBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_queued_bytes = 4 * MIB + 1,
        Slot::QueuedSubscriptionBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_encoded_event_bytes = 0,
        Slot::EncodedSubscriptionEventBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_encoded_event_bytes = MIB + 1,
        Slot::EncodedSubscriptionEventBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_queue_age = Duration::ZERO,
        Slot::SubscriptionQueueAge,
        ZeroDuration,
    );
    invalid_limit(
        |value| value.max_queue_age = over_seconds(60),
        Slot::SubscriptionQueueAge,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.send_no_progress_timeout = Duration::ZERO,
        Slot::SubscriptionSendNoProgressTimeout,
        ZeroDuration,
    );
    invalid_limit(
        |value| value.send_no_progress_timeout = over_seconds(60),
        Slot::SubscriptionSendNoProgressTimeout,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.setup_timeout = Duration::ZERO,
        Slot::SubscriptionSetupTimeout,
        ZeroDuration,
    );
    invalid_limit(
        |value| value.setup_timeout = over_seconds(60),
        Slot::SubscriptionSetupTimeout,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.event_timeout = Duration::ZERO,
        Slot::SubscriptionEventTimeout,
        ZeroDuration,
    );
    invalid_limit(
        |value| value.event_timeout = over_seconds(120),
        Slot::SubscriptionEventTimeout,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_endpoint_encoded_bytes = 0,
        Slot::EndpointSubscriptionEncodedBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_endpoint_encoded_bytes = 256 * MIB + 1,
        Slot::EndpointSubscriptionEncodedBytes,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_pending_operation_setups = 0,
        Slot::PendingOperationSetups,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_pending_operation_setups = 129,
        Slot::PendingOperationSetups,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_retained_metadata_bytes = 0,
        Slot::RetainedSubscriberMetadata,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.max_retained_metadata_bytes = 64 * KIB + 1,
        Slot::RetainedSubscriberMetadata,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.heartbeat_interval = Duration::ZERO,
        Slot::SubscriptionHeartbeatInterval,
        ZeroDuration,
    );
    invalid_limit(
        |value| value.heartbeat_interval = Duration::from_millis(999),
        Slot::SubscriptionHeartbeatInterval,
        OutsideHardBound,
    );
    invalid_limit(
        |value| value.heartbeat_interval = over_seconds(60),
        Slot::SubscriptionHeartbeatInterval,
        OutsideHardBound,
    );
}

#[test]
fn every_protocol_limit_rejects_zero_and_one_over_maximum() {
    use ConfigurationSlot as Slot;

    invalid_protocol(
        |value| value.max_active_operations_per_connection = 0,
        Slot::ActiveOperationsPerConnection,
    );
    invalid_protocol(
        |value| value.max_active_operations_per_connection = 129,
        Slot::ActiveOperationsPerConnection,
    );
    invalid_protocol(
        |value| value.max_websocket_connections = 0,
        Slot::WebSocketConnections,
    );
    invalid_protocol(
        |value| value.max_websocket_connections = 513,
        Slot::WebSocketConnections,
    );
    invalid_protocol(
        |value| value.max_pending_setups_per_connection = 0,
        Slot::PendingSetupsPerConnection,
    );
    invalid_protocol(
        |value| value.max_pending_setups_per_connection = 33,
        Slot::PendingSetupsPerConnection,
    );
}

#[test]
fn arithmetic_overflow_and_cross_limit_inconsistency_are_distinct() {
    let queue_mismatch = SubscriptionLimits {
        max_queued_bytes: 1,
        ..SubscriptionLimits::default()
    };
    let error = diagnostic(build(
        queue_mismatch,
        SubscriptionProtocolSettings::default(),
    ));
    assert_eq!(
        error.slot(),
        ConfigurationSlot::EncodedSubscriptionEventBytes
    );
    assert_eq!(
        error.violation(),
        ConfigurationViolation::InconsistentLimits
    );

    let endpoint_mismatch = SubscriptionLimits {
        max_endpoint_encoded_bytes: 1,
        ..SubscriptionLimits::default()
    };
    let error = diagnostic(build(
        endpoint_mismatch,
        SubscriptionProtocolSettings::default(),
    ));
    assert_eq!(
        error.slot(),
        ConfigurationSlot::EncodedSubscriptionEventBytes
    );
    assert_eq!(
        error.violation(),
        ConfigurationViolation::InconsistentLimits
    );

    let overflow = SubscriptionLimits {
        max_queued_events: usize::MAX,
        max_encoded_event_bytes: usize::MAX,
        ..SubscriptionLimits::default()
    };
    let error = diagnostic(build(overflow, SubscriptionProtocolSettings::default()));
    assert_eq!(error.slot(), ConfigurationSlot::QueuedSubscriptionBytes);
    assert_eq!(
        error.violation(),
        ConfigurationViolation::ArithmeticOverflow
    );
}

fn invalid_limit(
    update: impl FnOnce(&mut SubscriptionLimits),
    slot: ConfigurationSlot,
    violation: ConfigurationViolation,
) {
    let mut limits = SubscriptionLimits::default();
    update(&mut limits);
    let error = diagnostic(build(limits, SubscriptionProtocolSettings::default()));
    assert_eq!(error.slot(), slot);
    assert_eq!(error.violation(), violation);
    assert_eq!(error.origin(), ConfigurationOrigin::User);
}

fn invalid_protocol(
    update: impl FnOnce(&mut SubscriptionProtocolSettings),
    slot: ConfigurationSlot,
) {
    let mut protocol = SubscriptionProtocolSettings::default();
    update(&mut protocol);
    let error = diagnostic(build(SubscriptionLimits::default(), protocol));
    assert_eq!(error.slot(), slot);
    assert_eq!(error.violation(), ConfigurationViolation::OutsideHardBound);
    assert_eq!(error.origin(), ConfigurationOrigin::User);
}

fn over_seconds(seconds: u64) -> Duration {
    Duration::from_secs(seconds) + Duration::from_nanos(1)
}
