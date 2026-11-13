//! Final validation for subscription delivery and protocol groups.

use std::time::Duration;

use crate::build_support::{
    ConfigurationConstraint, ConfigurationDiagnostic, ConfigurationOperand, ConfigurationSlot,
    ConfigurationValue, ConfigurationViolation, EndpointBuildError,
};
use crate::configuration::ConfigurationOrigin;
use crate::draft::Draft;
use crate::subscription::request::MetadataRetentionLimit;
use crate::subscription::settings::{
    SubscriptionLimits as Limits, SubscriptionProtocolSettings as Protocol,
    ValidatedSubscriptionSettings,
};

pub(super) fn configuration<C>(
    draft: &Draft<C>,
) -> Result<ValidatedSubscriptionSettings, EndpointBuildError> {
    let limits = draft.subscription_limits.value;
    let limits_origin = draft.subscription_limits.origin();
    // Keep the established overflow precedence, even for out-of-range operands.
    // The diagnostic retains operands; it never computes an overflowed product.
    limits
        .max_queued_events
        .checked_mul(limits.max_encoded_event_bytes)
        .ok_or_else(|| {
            invalid(
                ConfigurationSlot::QueuedSubscriptionBytes,
                limits_origin,
                ConfigurationViolation::ArithmeticOverflow,
                ConfigurationConstraint::MultiplicationOverflow {
                    left: ConfigurationOperand::new(
                        ConfigurationSlot::QueuedSubscriptionEvents,
                        ConfigurationValue::Count(limits.max_queued_events),
                        limits_origin,
                    ),
                    right: ConfigurationOperand::new(
                        ConfigurationSlot::EncodedSubscriptionEventBytes,
                        ConfigurationValue::Bytes(limits.max_encoded_event_bytes),
                        limits_origin,
                    ),
                },
            )
        })?;
    count(
        limits.max_active_subscriptions,
        Limits::MAX_ACTIVE_SUBSCRIPTIONS,
        ConfigurationSlot::ActiveSubscriptions,
        limits_origin,
    )?;
    count(
        limits.max_queued_events,
        Limits::MAX_QUEUED_EVENTS,
        ConfigurationSlot::QueuedSubscriptionEvents,
        limits_origin,
    )?;
    bytes(
        limits.max_queued_bytes,
        Limits::MAX_QUEUED_BYTES,
        ConfigurationSlot::QueuedSubscriptionBytes,
        limits_origin,
    )?;
    bytes(
        limits.max_encoded_event_bytes,
        Limits::MAX_ENCODED_EVENT_BYTES,
        ConfigurationSlot::EncodedSubscriptionEventBytes,
        limits_origin,
    )?;
    duration(
        limits.max_queue_age,
        Limits::MIN_TIMEOUT,
        Limits::MAX_QUEUE_AGE,
        ConfigurationSlot::SubscriptionQueueAge,
        limits_origin,
    )?;
    duration(
        limits.send_no_progress_timeout,
        Limits::MIN_TIMEOUT,
        Limits::MAX_SEND_NO_PROGRESS_TIMEOUT,
        ConfigurationSlot::SubscriptionSendNoProgressTimeout,
        limits_origin,
    )?;
    duration(
        limits.setup_timeout,
        Limits::MIN_TIMEOUT,
        Limits::MAX_SETUP_TIMEOUT,
        ConfigurationSlot::SubscriptionSetupTimeout,
        limits_origin,
    )?;
    duration(
        limits.event_timeout,
        Limits::MIN_TIMEOUT,
        Limits::MAX_EVENT_TIMEOUT,
        ConfigurationSlot::SubscriptionEventTimeout,
        limits_origin,
    )?;
    bytes(
        limits.max_endpoint_encoded_bytes,
        Limits::MAX_ENDPOINT_ENCODED_BYTES,
        ConfigurationSlot::EndpointSubscriptionEncodedBytes,
        limits_origin,
    )?;
    count(
        limits.max_pending_operation_setups,
        Limits::MAX_PENDING_OPERATION_SETUPS,
        ConfigurationSlot::PendingOperationSetups,
        limits_origin,
    )?;
    MetadataRetentionLimit::new(limits.max_retained_metadata_bytes).map_err(|_| {
        invalid(
            ConfigurationSlot::RetainedSubscriberMetadata,
            limits_origin,
            ConfigurationViolation::OutsideHardBound,
            ConfigurationConstraint::Range {
                supplied: ConfigurationValue::Bytes(limits.max_retained_metadata_bytes),
                minimum: ConfigurationValue::Bytes(Limits::MIN_BYTES),
                maximum: ConfigurationValue::Bytes(Limits::MAX_RETAINED_METADATA_BYTES),
            },
        )
    })?;
    duration(
        limits.heartbeat_interval,
        Limits::MIN_HEARTBEAT_INTERVAL,
        Limits::MAX_HEARTBEAT_INTERVAL,
        ConfigurationSlot::SubscriptionHeartbeatInterval,
        limits_origin,
    )?;
    // Queue fit wins deterministically if both capacities are too small.
    for (slot, capacity) in [
        (
            ConfigurationSlot::QueuedSubscriptionBytes,
            limits.max_queued_bytes,
        ),
        (
            ConfigurationSlot::EndpointSubscriptionEncodedBytes,
            limits.max_endpoint_encoded_bytes,
        ),
    ] {
        if limits.max_encoded_event_bytes > capacity {
            return Err(invalid(
                ConfigurationSlot::EncodedSubscriptionEventBytes,
                limits_origin,
                ConfigurationViolation::InconsistentLimits,
                ConfigurationConstraint::Fit {
                    supplied: limits.max_encoded_event_bytes,
                    capacity: ConfigurationOperand::new(
                        slot,
                        ConfigurationValue::Bytes(capacity),
                        limits_origin,
                    ),
                },
            ));
        }
    }

    let protocol = draft.subscription_protocol.value;
    let protocol_origin = draft.subscription_protocol.origin();
    count(
        protocol.max_active_operations_per_connection,
        Protocol::MAX_ACTIVE_OPERATIONS_PER_CONNECTION,
        ConfigurationSlot::ActiveOperationsPerConnection,
        protocol_origin,
    )?;
    count(
        protocol.max_websocket_connections,
        Protocol::MAX_WEBSOCKET_CONNECTIONS,
        ConfigurationSlot::WebSocketConnections,
        protocol_origin,
    )?;
    count(
        protocol.max_pending_setups_per_connection,
        Protocol::MAX_PENDING_SETUPS_PER_CONNECTION,
        ConfigurationSlot::PendingSetupsPerConnection,
        protocol_origin,
    )?;
    Ok(ValidatedSubscriptionSettings::new(limits, protocol))
}

fn count(
    value: usize,
    maximum: usize,
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
) -> Result<(), EndpointBuildError> {
    numeric_range(
        value,
        Limits::MIN_COUNT,
        maximum,
        ConfigurationValue::Count,
        slot,
        origin,
    )
}

fn bytes(
    value: usize,
    maximum: usize,
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
) -> Result<(), EndpointBuildError> {
    numeric_range(
        value,
        Limits::MIN_BYTES,
        maximum,
        ConfigurationValue::Bytes,
        slot,
        origin,
    )
}

fn numeric_range(
    value: usize,
    minimum: usize,
    maximum: usize,
    unit: fn(usize) -> ConfigurationValue,
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
) -> Result<(), EndpointBuildError> {
    if !(minimum..=maximum).contains(&value) {
        return Err(invalid(
            slot,
            origin,
            ConfigurationViolation::OutsideHardBound,
            ConfigurationConstraint::Range {
                supplied: unit(value),
                minimum: unit(minimum),
                maximum: unit(maximum),
            },
        ));
    }
    Ok(())
}

fn duration(
    value: Duration,
    minimum: Duration,
    maximum: Duration,
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
) -> Result<(), EndpointBuildError> {
    let violation = if value.is_zero() {
        Some(ConfigurationViolation::ZeroDuration)
    } else if value < minimum || value > maximum {
        Some(ConfigurationViolation::OutsideHardBound)
    } else {
        None
    };
    match violation {
        Some(violation) => Err(invalid(
            slot,
            origin,
            violation,
            ConfigurationConstraint::Range {
                supplied: ConfigurationValue::Duration(value),
                minimum: ConfigurationValue::Duration(minimum),
                maximum: ConfigurationValue::Duration(maximum),
            },
        )),
        None => Ok(()),
    }
}

const fn invalid(
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
    violation: ConfigurationViolation,
    constraint: ConfigurationConstraint,
) -> EndpointBuildError {
    EndpointBuildError::InvalidConfiguration(
        ConfigurationDiagnostic::new(slot, origin, violation).with_constraint(constraint),
    )
}
