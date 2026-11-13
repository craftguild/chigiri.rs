//! Distinguish provider-timeout diagnostics from algorithm-policy diagnostics.

use std::time::Duration;

use super::rate_support::TransportKey;

#[test]
fn limit_07_reports_distinct_timeout_and_algorithm_duration_errors() {
    for timeout in [Duration::ZERO, Duration::MAX] {
        assert!(matches!(
            chigiri::rate::transport(TransportKey)
                .token_bucket()
                .provider_timeout(timeout),
            Err(chigiri::rate::RateStageBuildError::InvalidProviderTimeout(
                _
            ))
        ));
        assert!(matches!(
            chigiri::rate::transport(TransportKey)
                .token_bucket()
                .refill(1, timeout),
            Err(chigiri::rate::RateStageBuildError::InvalidPolicy(_))
        ));
        assert!(matches!(
            chigiri::rate::transport(TransportKey)
                .fixed_window()
                .window(timeout),
            Err(chigiri::rate::RateStageBuildError::InvalidPolicy(_))
        ));
    }
}
