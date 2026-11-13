//! Every configurable numeric field has exact, unit-bearing boundary evidence.

use std::time::Duration;

use chigiri::{
    ConfigurationSlot as Slot, ConfigurationValue as Value, ConfigurationViolation as Violation,
    SubscriptionLimits as Limits, SubscriptionProtocolSettings as Protocol,
};

use super::{assert_range, build_with_parity, diagnostic};

struct Numeric<T> {
    set: fn(&mut T, usize),
    slot: Slot,
    maximum: usize,
    unit: fn(usize) -> Value,
}

#[test]
fn delivery_numeric_fields_accept_inclusive_bounds_and_report_zero_and_over_maximum() {
    let fields = [
        Numeric {
            set: |v: &mut Limits, n| v.max_active_subscriptions = n,
            slot: Slot::ActiveSubscriptions,
            maximum: 1_024,
            unit: Value::Count,
        },
        Numeric {
            set: |v, n| v.max_queued_events = n,
            slot: Slot::QueuedSubscriptionEvents,
            maximum: 1_024,
            unit: Value::Count,
        },
        Numeric {
            set: |v, n| v.max_queued_bytes = n,
            slot: Slot::QueuedSubscriptionBytes,
            maximum: 4_194_304,
            unit: Value::Bytes,
        },
        Numeric {
            set: |v, n| v.max_encoded_event_bytes = n,
            slot: Slot::EncodedSubscriptionEventBytes,
            maximum: 1_048_576,
            unit: Value::Bytes,
        },
        Numeric {
            set: |v, n| v.max_endpoint_encoded_bytes = n,
            slot: Slot::EndpointSubscriptionEncodedBytes,
            maximum: 268_435_456,
            unit: Value::Bytes,
        },
        Numeric {
            set: |v, n| v.max_pending_operation_setups = n,
            slot: Slot::PendingOperationSetups,
            maximum: 128,
            unit: Value::Count,
        },
        Numeric {
            set: |v, n| v.max_retained_metadata_bytes = n,
            slot: Slot::RetainedSubscriberMetadata,
            maximum: 65_536,
            unit: Value::Bytes,
        },
    ];
    for field in fields {
        for value in [0, 1, field.maximum, field.maximum + 1] {
            let mut limits = Limits {
                max_encoded_event_bytes: 1,
                max_queued_bytes: 4_194_304,
                ..Limits::default()
            };
            (field.set)(&mut limits, value);
            let result = build_with_parity(limits, Protocol::default());
            if value == 1 || value == field.maximum {
                assert!(
                    result.is_ok(),
                    "{:?} = {value}: {:?}",
                    field.slot,
                    result.err()
                );
            } else {
                assert_range(
                    diagnostic(result),
                    field.slot,
                    (field.unit)(value),
                    (field.unit)(1),
                    (field.unit)(field.maximum),
                    Violation::OutsideHardBound,
                );
            }
        }
    }
}

#[test]
fn protocol_numeric_fields_keep_count_units_and_inclusive_bounds() {
    let fields = [
        Numeric {
            set: |v: &mut Protocol, n| v.max_active_operations_per_connection = n,
            slot: Slot::ActiveOperationsPerConnection,
            maximum: 128,
            unit: Value::Count,
        },
        Numeric {
            set: |v, n| v.max_websocket_connections = n,
            slot: Slot::WebSocketConnections,
            maximum: 512,
            unit: Value::Count,
        },
        Numeric {
            set: |v, n| v.max_pending_setups_per_connection = n,
            slot: Slot::PendingSetupsPerConnection,
            maximum: 32,
            unit: Value::Count,
        },
    ];
    for field in fields {
        for value in [0, 1, field.maximum, field.maximum + 1] {
            let mut protocol = Protocol::default();
            (field.set)(&mut protocol, value);
            let result = build_with_parity(Limits::default(), protocol);
            if value == 1 || value == field.maximum {
                assert!(result.is_ok());
            } else {
                assert_range(
                    diagnostic(result),
                    field.slot,
                    Value::Count(value),
                    Value::Count(1),
                    Value::Count(field.maximum),
                    Violation::OutsideHardBound,
                );
            }
        }
    }
}

struct Timed {
    set: fn(&mut Limits, Duration),
    slot: Slot,
    minimum: Duration,
    maximum: Duration,
}

#[test]
fn durations_include_zero_sub_heartbeat_nanosecond_edges_and_max_sentinel() {
    let fields = [
        Timed {
            set: |v, n| v.max_queue_age = n,
            slot: Slot::SubscriptionQueueAge,
            minimum: Duration::from_nanos(1),
            maximum: Duration::from_secs(60),
        },
        Timed {
            set: |v, n| v.send_no_progress_timeout = n,
            slot: Slot::SubscriptionSendNoProgressTimeout,
            minimum: Duration::from_nanos(1),
            maximum: Duration::from_secs(60),
        },
        Timed {
            set: |v, n| v.setup_timeout = n,
            slot: Slot::SubscriptionSetupTimeout,
            minimum: Duration::from_nanos(1),
            maximum: Duration::from_secs(60),
        },
        Timed {
            set: |v, n| v.event_timeout = n,
            slot: Slot::SubscriptionEventTimeout,
            minimum: Duration::from_nanos(1),
            maximum: Duration::from_secs(120),
        },
        Timed {
            set: |v, n| v.heartbeat_interval = n,
            slot: Slot::SubscriptionHeartbeatInterval,
            minimum: Duration::from_secs(1),
            maximum: Duration::from_secs(60),
        },
    ];
    for field in fields {
        for value in [
            Duration::ZERO,
            field.minimum - Duration::from_nanos(1),
            field.minimum,
            field.maximum,
            field.maximum + Duration::from_nanos(1),
            Duration::MAX,
        ] {
            let mut limits = Limits::default();
            (field.set)(&mut limits, value);
            let result = build_with_parity(limits, Protocol::default());
            if value == field.minimum || value == field.maximum {
                assert!(result.is_ok());
            } else {
                let violation = if value.is_zero() {
                    Violation::ZeroDuration
                } else {
                    Violation::OutsideHardBound
                };
                assert_range(
                    diagnostic(result),
                    field.slot,
                    Value::Duration(value),
                    Value::Duration(field.minimum),
                    Value::Duration(field.maximum),
                    violation,
                );
            }
        }
    }
}
