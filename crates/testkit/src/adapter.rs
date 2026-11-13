//! Runtime-neutral harness for running identical scenarios through Hyper and Axum.

use std::fmt;

use chigiri_http::BoxFuture;

/// Closed set of adapter capabilities required by the production lifecycle suite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterScenario {
    PhaseOrder,
    TransportRejectBeforeBody,
    SourceReject,
    ContextReject,
    OperationReject,
    PhaseTimeoutDrop,
    CsrfSimpleRequest,
    ResponseHeaderAuthority,
}

/// Complete production lifecycle scenario set.
pub const ADAPTER_SCENARIOS: &[AdapterScenario] = &[
    AdapterScenario::PhaseOrder,
    AdapterScenario::TransportRejectBeforeBody,
    AdapterScenario::SourceReject,
    AdapterScenario::ContextReject,
    AdapterScenario::OperationReject,
    AdapterScenario::PhaseTimeoutDrop,
    AdapterScenario::CsrfSimpleRequest,
    AdapterScenario::ResponseHeaderAuthority,
];

/// Adapter observation captured for one scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterConformanceError {
    scenario: AdapterScenario,
    message: &'static str,
}

impl AdapterConformanceError {
    pub const fn new(scenario: AdapterScenario, message: &'static str) -> Self {
        Self { scenario, message }
    }
    pub const fn scenario(&self) -> AdapterScenario {
        self.scenario
    }
}

impl fmt::Display for AdapterConformanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for AdapterConformanceError {}

/// Object-safe adapter that runs shared scenarios on its own runtime.
pub trait LifecycleHarness: Send + Sync + 'static {
    fn run(&self, scenario: AdapterScenario) -> BoxFuture<'_, Result<(), AdapterConformanceError>>;
}

/// Applies the public scenario set to one adapter harness in sequence.
pub async fn assert_lifecycle_conformance(
    harness: &dyn LifecycleHarness,
) -> Result<(), AdapterConformanceError> {
    for scenario in ADAPTER_SCENARIOS {
        harness.run(*scenario).await?;
    }
    Ok(())
}
