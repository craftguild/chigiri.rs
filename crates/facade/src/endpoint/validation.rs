//! Fail-closed validation of the final component graph before construction.

use std::time::Duration;

use crate::build_support::{ConfigurationSlot, ConfigurationViolation, EndpointBuildError};
use crate::configuration::ConfigurationOrigin;
use crate::draft::{Draft, Slot};

use super::request_identity;

#[cfg(test)]
mod subscription_order;
mod subscriptions;

const MAX_HTTP_BYTES: usize = 1024 * 1024 * 1024;
const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) fn configuration<C>(
    draft: &mut Draft<C>,
) -> Result<crate::subscription::settings::ValidatedSubscriptionSettings, EndpointBuildError> {
    let limits = draft.limits.value;
    let nonzero_limits = [
        limits.max_document_bytes,
        limits.max_parser_nesting,
        limits.max_operation_depth,
        limits.max_expanded_fields,
        limits.max_aliases,
        limits.max_variables,
        limits.max_directives,
        limits.max_diagnostics,
        limits.max_concurrent_fields,
        limits.max_list_items_per_field,
        limits.max_custom_scalar_output_depth,
        limits.max_output_nodes,
    ];
    if nonzero_limits.contains(&usize::MAX) {
        return Err(invalid(
            ConfigurationSlot::ExecutionLimits,
            draft.limits.origin(),
            ConfigurationViolation::Unbounded,
        ));
    }
    let cache = draft.cache.value;
    if (cache.max_entries == 0) != (cache.max_query_bytes == 0) {
        return Err(invalid(
            ConfigurationSlot::DocumentCache,
            draft.cache.origin(),
            ConfigurationViolation::PartiallyDisabled,
        ));
    }
    if cache.max_entries == usize::MAX || cache.max_query_bytes == usize::MAX {
        return Err(invalid(
            ConfigurationSlot::DocumentCache,
            draft.cache.origin(),
            ConfigurationViolation::Unbounded,
        ));
    }
    validate_bytes(
        &draft.body_limit,
        ConfigurationSlot::RequestBodyByteLimit,
        1,
    )?;
    validate_bytes(
        &draft.response_limit,
        ConfigurationSlot::ResponseByteLimit,
        128,
    )?;
    let deadlines = [
        (ConfigurationSlot::BodyTimeout, &draft.body_timeout),
        (
            ConfigurationSlot::TransportAdmissionTimeout,
            &draft.transport_timeout,
        ),
        (
            ConfigurationSlot::SourceResolutionTimeout,
            &draft.source_timeout,
        ),
        (
            ConfigurationSlot::DecodedRequestTimeout,
            &draft.decoded_timeout,
        ),
        (ConfigurationSlot::ContextTimeout, &draft.context_timeout),
        (
            ConfigurationSlot::ExecutionTimeout,
            &draft.execution_timeout,
        ),
        (
            ConfigurationSlot::ResponseHookTimeout,
            &draft.response_hook_timeout,
        ),
        (ConfigurationSlot::OverallTimeout, &draft.overall_timeout),
    ];
    for (name, slot) in deadlines {
        validate_deadline(slot, name)?;
    }
    if draft
        .safety_overrides
        .iter()
        .any(|entry| entry.reason().trim().is_empty())
    {
        return Err(invalid(
            ConfigurationSlot::SafetyOverrideReason,
            ConfigurationOrigin::User,
            ConfigurationViolation::EmptyReason,
        ));
    }
    let subscriptions = subscriptions::configuration(draft)?;
    request_identity::initialize(&mut draft.request_ids)?;
    Ok(subscriptions)
}

fn validate_bytes(
    slot: &Slot<usize>,
    name: ConfigurationSlot,
    minimum: usize,
) -> Result<(), EndpointBuildError> {
    if slot.value < minimum || slot.value > MAX_HTTP_BYTES {
        return Err(invalid(
            name,
            slot.origin(),
            ConfigurationViolation::OutsideHardBound,
        ));
    }
    Ok(())
}

fn validate_deadline(
    slot: &Slot<Duration>,
    name: ConfigurationSlot,
) -> Result<(), EndpointBuildError> {
    let violation = if slot.value.is_zero() {
        Some(ConfigurationViolation::ZeroDuration)
    } else if slot.value > MAX_PHASE_TIMEOUT {
        Some(ConfigurationViolation::UnschedulableDuration)
    } else {
        None
    };
    match violation {
        Some(violation) => Err(invalid(name, slot.origin(), violation)),
        None => Ok(()),
    }
}

const fn invalid(
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
    violation: ConfigurationViolation,
) -> EndpointBuildError {
    EndpointBuildError::invalid(slot, origin, violation)
}

#[cfg(test)]
mod tests {
    use super::{ConfigurationSlot, ConfigurationViolation, Slot, validate_deadline};
    use crate::ConfigurationOrigin;

    #[test]
    fn baseline_derived_inconsistency_also_retains_origin() {
        let slot = Slot::baseline(std::time::Duration::ZERO);
        let error = validate_deadline(&slot, ConfigurationSlot::ExecutionTimeout).unwrap_err();
        let crate::EndpointBuildError::InvalidConfiguration(diagnostic) = error else {
            panic!("expected a schema composition diagnostic");
        };
        assert_eq!(diagnostic.origin(), ConfigurationOrigin::Baseline);
        assert_eq!(diagnostic.slot(), ConfigurationSlot::ExecutionTimeout);
        assert_eq!(diagnostic.violation(), ConfigurationViolation::ZeroDuration);
    }
}
