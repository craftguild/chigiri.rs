//! Shared conformance entry points for third-party providers.

mod operation;
mod rate;

use std::fmt;
use std::task::{Context, Poll, Waker};

use chigiri_http::BoxFuture;

pub use operation::{
    OperationProviderCase, OperationProviderConformance, assert_operation_provider_conformance,
};
pub use rate::{
    ExpectedRateDecision, RateProviderCase, RateProviderConcurrencyCase, RateProviderConformance,
    RateProviderStateCase, RateProviderTimeCase, RateProviderTimeControl,
    assert_rate_provider_concurrency_conformance, assert_rate_provider_conformance,
    assert_rate_provider_nonblocking_conformance, assert_rate_provider_state_conformance,
    assert_rate_provider_time_conformance,
};

use crate::DropProbeState;

/// Poll a pending provider future directly and verify it is dropped on timeout or cancellation.
pub fn assert_provider_future_is_droppable<T>(
    mut future: BoxFuture<'_, T>,
    state: &DropProbeState,
) -> Result<(), ProviderConformanceError> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    if !matches!(future.as_mut().poll(&mut context), Poll::Pending) {
        return Err(ProviderConformanceError::new(
            "provider future did not remain pending",
        ));
    }
    drop(future);
    if state.polls() != 1 || !state.was_dropped() {
        return Err(ProviderConformanceError::new(
            "provider future did not expose poll and drop",
        ));
    }
    Ok(())
}

/// Provider failure used by conformance tests without exposing provider internals.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderConformanceError {
    message: &'static str,
}

impl ProviderConformanceError {
    pub(crate) const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for ProviderConformanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ProviderConformanceError {}
