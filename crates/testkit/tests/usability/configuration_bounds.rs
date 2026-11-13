//! Usually, the setter checks that it does not create unbounded or timer-indefinite configurations.

use std::time::Duration;

use super::support::resolvers;

const MAX_HTTP_BYTES: usize = 1024 * 1024 * 1024;
const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

fn endpoint_with(
    edit: impl FnOnce(&mut chigiri::Components<'_, (), chigiri::ContextPending>),
) -> Result<chigiri::Endpoint<()>, chigiri::EndpointBuildError> {
    chigiri::Endpoint::builder(resolvers())
        .inject(edit)
        .without_context()
        .build()
}

#[test]
fn inj_09_enforces_hard_bounds_for_both_byte_limits() {
    for bytes in [MAX_HTTP_BYTES + 1, usize::MAX] {
        assert!(
            endpoint_with(|components| {
                components.http().body_limit(bytes);
            })
            .is_err()
        );
        assert!(
            endpoint_with(|components| {
                components.http().response_byte_limit(bytes);
            })
            .is_err()
        );
    }
}

#[test]
fn inj_09_rejects_duration_max_for_all_phase_deadlines() {
    macro_rules! assert_rejected {
        ($($setter:ident),+ $(,)?) => {$({
            assert!(endpoint_with(|components| {
                components.http().$setter(Duration::MAX);
            }).is_err(), "{} accepted Duration::MAX", stringify!($setter));
        })+};
    }
    assert_rejected!(
        body_timeout,
        transport_admission_timeout,
        source_resolution_timeout,
        decoded_request_timeout,
        context_timeout,
        execution_timeout,
        response_hook_timeout,
        overall_timeout,
    );
}

#[test]
fn inj_09_enforces_standard_hard_boundary_values() {
    let endpoint = endpoint_with(|components| {
        components
            .http()
            .body_limit(MAX_HTTP_BYTES)
            .response_byte_limit(MAX_HTTP_BYTES)
            .body_timeout(MAX_PHASE_TIMEOUT)
            .transport_admission_timeout(MAX_PHASE_TIMEOUT)
            .source_resolution_timeout(MAX_PHASE_TIMEOUT)
            .decoded_request_timeout(MAX_PHASE_TIMEOUT)
            .context_timeout(MAX_PHASE_TIMEOUT)
            .execution_timeout(MAX_PHASE_TIMEOUT)
            .response_hook_timeout(MAX_PHASE_TIMEOUT)
            .overall_timeout(MAX_PHASE_TIMEOUT);
    });
    assert!(endpoint.is_ok());
}
