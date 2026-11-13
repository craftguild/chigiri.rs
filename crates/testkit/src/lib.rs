#![forbid(unsafe_code)]
//! Reusable conformance support for adapter and provider development dependencies.

mod adapter;
mod assertions;
mod catalog;
pub mod differential;
mod drop_probe;
mod provider;

pub use adapter::{
    ADAPTER_SCENARIOS, AdapterConformanceError, AdapterScenario, LifecycleHarness,
    assert_lifecycle_conformance,
};
pub use assertions::{GraphQlAssertionError, assert_error_code, assert_request_error};
pub use catalog::{
    CatalogImpactChange, CatalogImpactChangeKind, CatalogImpactEntry, CatalogImpactError,
    CatalogImpactSnapshot, diff_catalog_impact, validate_catalog_impact,
};
pub use drop_probe::{DropProbeState, PendingDropProbe};
pub use provider::{
    ExpectedRateDecision, OperationProviderCase, OperationProviderConformance,
    ProviderConformanceError, RateProviderCase, RateProviderConcurrencyCase,
    RateProviderConformance, RateProviderStateCase, RateProviderTimeCase, RateProviderTimeControl,
    assert_operation_provider_conformance, assert_provider_future_is_droppable,
    assert_rate_provider_concurrency_conformance, assert_rate_provider_conformance,
    assert_rate_provider_nonblocking_conformance, assert_rate_provider_state_conformance,
    assert_rate_provider_time_conformance,
};
