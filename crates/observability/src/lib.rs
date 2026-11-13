#![forbid(unsafe_code)]
//! Standard components for sending backend-neutral events to bounded nonblocking sinks.

mod id;
mod metrics;
mod queue;

pub use id::{
    DeterministicErrorIdFactory, DeterministicRequestIdFactory, RandomErrorIdFactory,
    RandomRequestIdFactory,
};
pub use metrics::{
    MetricLabel, MetricLabelError, MetricLabelKey, MetricLabels, MetricOperationKind,
};
pub use queue::{
    BoundedExporterQueue, ErrorRecord, ExporterReceiver, LifecycleExporter, LifecycleRecord,
    QueueBuildError, TryExport,
};

pub use chigiri_runtime::{Sampler, SamplingDecision};

/// The standard sampler that records every request.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlwaysRecordSampler;

impl Sampler for AlwaysRecordSampler {
    fn sample(&self, _: chigiri_runtime::RequestIdentity<'_>) -> SamplingDecision {
        SamplingDecision::Record
    }
}
