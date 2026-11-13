//! Verify final validation reports configuration slots without exposing values.

use std::time::Duration;

use chigiri::{
    ConfigurationDiagnostic, ConfigurationOrigin, ConfigurationSlot, ConfigurationViolation,
    DocumentCacheLimits, EndpointBuildError, ExecutionLimits,
};

use super::support::resolvers;

fn endpoint_with(
    edit: impl FnOnce(&mut chigiri::Components<'_, (), chigiri::ContextPending>),
) -> Result<chigiri::Endpoint<()>, EndpointBuildError> {
    chigiri::Endpoint::builder(resolvers())
        .inject(edit)
        .without_context()
        .build()
}

fn diagnostic(
    result: Result<chigiri::Endpoint<()>, EndpointBuildError>,
) -> ConfigurationDiagnostic {
    match result.err().unwrap() {
        EndpointBuildError::InvalidConfiguration(diagnostic) => diagnostic,
        other => panic!("expected a configuration diagnostic: {other:?}"),
    }
}

fn assert_diagnostic(
    diagnostic: ConfigurationDiagnostic,
    slot: ConfigurationSlot,
    violation: ConfigurationViolation,
) {
    assert_eq!(diagnostic.slot(), slot);
    assert_eq!(diagnostic.origin(), ConfigurationOrigin::User);
    assert_eq!(diagnostic.violation(), violation);
    assert_eq!(diagnostic.constraint(), None);
}

#[test]
fn distinguishes_body_and_response_byte_configuration_slots() {
    assert_diagnostic(
        diagnostic(endpoint_with(|components| {
            components.http().body_limit(0);
        })),
        ConfigurationSlot::RequestBodyByteLimit,
        ConfigurationViolation::OutsideHardBound,
    );
    assert_diagnostic(
        diagnostic(endpoint_with(|components| {
            components.http().response_byte_limit(0);
        })),
        ConfigurationSlot::ResponseByteLimit,
        ConfigurationViolation::OutsideHardBound,
    );
}

#[test]
fn diagnoses_each_deadline_configuration_slot_individually() {
    macro_rules! assert_deadline {
        ($setter:ident, $slot:expr) => {{
            assert_diagnostic(
                diagnostic(endpoint_with(|components| {
                    components.http().$setter(Duration::ZERO);
                })),
                $slot,
                ConfigurationViolation::ZeroDuration,
            );
            assert_diagnostic(
                diagnostic(endpoint_with(|components| {
                    components.http().$setter(Duration::MAX);
                })),
                $slot,
                ConfigurationViolation::UnschedulableDuration,
            );
        }};
    }

    assert_deadline!(body_timeout, ConfigurationSlot::BodyTimeout);
    assert_deadline!(
        transport_admission_timeout,
        ConfigurationSlot::TransportAdmissionTimeout
    );
    assert_deadline!(
        source_resolution_timeout,
        ConfigurationSlot::SourceResolutionTimeout
    );
    assert_deadline!(
        decoded_request_timeout,
        ConfigurationSlot::DecodedRequestTimeout
    );
    assert_deadline!(context_timeout, ConfigurationSlot::ContextTimeout);
    assert_deadline!(execution_timeout, ConfigurationSlot::ExecutionTimeout);
    assert_deadline!(
        response_hook_timeout,
        ConfigurationSlot::ResponseHookTimeout
    );
    assert_deadline!(overall_timeout, ConfigurationSlot::OverallTimeout);
}

#[test]
fn diagnoses_complex_configuration_slots_and_safety_override_reason() {
    let mut limits = ExecutionLimits::production();
    limits.max_document_bytes = usize::MAX;
    assert_diagnostic(
        diagnostic(endpoint_with(|components| {
            components.execution().limits(limits);
        })),
        ConfigurationSlot::ExecutionLimits,
        ConfigurationViolation::Unbounded,
    );

    assert_diagnostic(
        diagnostic(endpoint_with(|components| {
            components.execution().document_cache(DocumentCacheLimits {
                max_entries: 0,
                max_query_bytes: 1,
            });
        })),
        ConfigurationSlot::DocumentCache,
        ConfigurationViolation::PartiallyDisabled,
    );
    for limits in [
        DocumentCacheLimits {
            max_entries: usize::MAX,
            max_query_bytes: 1,
        },
        DocumentCacheLimits {
            max_entries: 1,
            max_query_bytes: usize::MAX,
        },
    ] {
        assert_diagnostic(
            diagnostic(endpoint_with(|components| {
                components.execution().document_cache(limits);
            })),
            ConfigurationSlot::DocumentCache,
            ConfigurationViolation::Unbounded,
        );
    }
    assert_diagnostic(
        diagnostic(endpoint_with(|components| {
            components
                .execution()
                .dangerously_enable_introspection("   ");
        })),
        ConfigurationSlot::SafetyOverrideReason,
        ConfigurationViolation::EmptyReason,
    );
}

#[test]
fn diagnostic_output_omits_configured_values_and_reasons() {
    let error = endpoint_with(|components| {
        components.http().body_limit(1_073_741_825);
    })
    .err()
    .unwrap();
    let debug = format!("{error:?}");
    let display = error.to_string();
    for output in [&debug, &display] {
        assert!(output.contains("RequestBodyByteLimit"));
        assert!(output.contains("User"));
        assert!(output.contains("OutsideHardBound"));
        assert!(!output.contains("1073741825"));
    }
    assert!(std::error::Error::source(&error).is_none());
}
