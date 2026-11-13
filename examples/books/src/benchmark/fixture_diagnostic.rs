//! Diagnostic profile isolating observer overhead by factor.

use std::sync::Arc;

use chigiri::{
    LifecycleEvent, LifecycleObserver, RequestId, RequestIdFactory, RequestIdFactoryError,
};
use chigiri_observability::{LifecycleExporter, RandomRequestIdFactory};

use super::{Fixture, OBSERVER_CAPACITY, builder};
use crate::Catalog;
use crate::benchmark::model::ProfileDefinition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::benchmark) enum DiagnosticProfile {
    NoopObserver,
    QueueOnly,
    RandomRequestId,
    TelemetryClone,
    FullObserver,
}

impl DiagnosticProfile {
    pub(in crate::benchmark) const ALL: [Self; 5] = [
        Self::NoopObserver,
        Self::QueueOnly,
        Self::RandomRequestId,
        Self::TelemetryClone,
        Self::FullObserver,
    ];

    pub(in crate::benchmark) const fn name(self) -> &'static str {
        match self {
            Self::NoopObserver => "diagnostic-noop-observer",
            Self::QueueOnly => "diagnostic-queue-only",
            Self::RandomRequestId => "diagnostic-random-request-id",
            Self::TelemetryClone => "diagnostic-telemetry-clone",
            Self::FullObserver => "diagnostic-full-observer",
        }
    }
}

pub(in crate::benchmark) fn diagnostic_profile_definitions() -> Vec<ProfileDefinition> {
    DiagnosticProfile::ALL
        .into_iter()
        .map(|profile| ProfileDefinition {
            name: profile.name().to_owned(),
            includes: includes(profile)
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            excludes: excludes(profile)
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        })
        .collect()
}

fn includes(profile: DiagnosticProfile) -> &'static [&'static str] {
    match profile {
        DiagnosticProfile::NoopObserver => &["lifecycle event construction", "no-op callback"],
        DiagnosticProfile::QueueOnly => &["lifecycle event construction", "bounded observer queue"],
        DiagnosticProfile::RandomRequestId => &[
            "seeded keyed-hash request ID",
            "request telemetry propagation",
        ],
        DiagnosticProfile::TelemetryClone => &[
            "fixed request ID",
            "request telemetry propagation and cloning",
        ],
        DiagnosticProfile::FullObserver => {
            &["request/error ID", "sampling", "bounded observer queue"]
        }
    }
}

fn excludes(profile: DiagnosticProfile) -> &'static [&'static str] {
    match profile {
        DiagnosticProfile::NoopObserver => &["queue", "request/error ID", "sampling"],
        DiagnosticProfile::QueueOnly => &["request/error ID", "sampling"],
        DiagnosticProfile::RandomRequestId => &["lifecycle observer", "error ID", "sampling"],
        DiagnosticProfile::TelemetryClone => &[
            "request ID derivation",
            "lifecycle observer",
            "error ID",
            "sampling",
        ],
        DiagnosticProfile::FullObserver => &["operation source", "rate"],
    }
}

impl Fixture {
    pub(in crate::benchmark) fn new_diagnostic(
        profile: DiagnosticProfile,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let catalog = Arc::new(Catalog::seeded());
        let (endpoint, receivers) = match profile {
            DiagnosticProfile::NoopObserver => (
                builder(true, Arc::clone(&catalog))
                    .inject(|components| {
                        components.observability().lifecycle_observer(NoopObserver);
                    })
                    .build()?,
                None,
            ),
            DiagnosticProfile::QueueOnly => {
                let (observer, lifecycle, errors) = LifecycleExporter::new(OBSERVER_CAPACITY)?;
                (
                    builder(true, Arc::clone(&catalog))
                        .inject(|components| {
                            components.observability().lifecycle_observer(observer);
                        })
                        .build()?,
                    Some((lifecycle, errors)),
                )
            }
            DiagnosticProfile::RandomRequestId => {
                let request_ids = RandomRequestIdFactory::new()?;
                (
                    builder(true, Arc::clone(&catalog))
                        .inject(|components| {
                            components.observability().request_id_factory(request_ids);
                        })
                        .build()?,
                    None,
                )
            }
            DiagnosticProfile::TelemetryClone => {
                let request_ids = FixedRequestIdFactory::new()?;
                (
                    builder(true, Arc::clone(&catalog))
                        .inject(|components| {
                            components.observability().request_id_factory(request_ids);
                        })
                        .build()?,
                    None,
                )
            }
            DiagnosticProfile::FullObserver => super::observer_fixture(true, Arc::clone(&catalog))?,
        };
        Ok(Self::from_parts(
            profile.name(),
            catalog,
            endpoint,
            receivers,
        ))
    }
}

#[derive(Clone, Copy)]
struct NoopObserver;

impl LifecycleObserver for NoopObserver {
    fn observe(&self, _: LifecycleEvent<'_>) {}
}

#[derive(Clone)]
struct FixedRequestIdFactory(RequestId);

impl FixedRequestIdFactory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self(RequestId::new("benchmark-request-id")?))
    }
}

impl RequestIdFactory for FixedRequestIdFactory {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        Ok(self.0.clone())
    }
}
