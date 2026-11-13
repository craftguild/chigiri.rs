//! Metric-series labels restricted to closed keys and bounded values.

use std::fmt;

use chigiri_runtime::{LifecyclePhase, SchemaDigest};

const LABELS: usize = 8;
const LABEL_VALUE_BYTES: usize = 64;

/// Closed set of label keys allowed by the metric contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetricLabelKey {
    OperationKind,
    RegisteredOperationKey,
    SchemaDigest,
    LifecyclePhase,
    ResultCode,
    Adapter,
    Provider,
}

/// Closed set of operation kinds allowed in metric series.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetricOperationKind {
    Query,
    Mutation,
    Subscription,
}

/// A bounded metric label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricLabel {
    key: MetricLabelKey,
    value: String,
}

impl MetricLabel {
    pub fn operation_kind(kind: MetricOperationKind) -> Self {
        let value = match kind {
            MetricOperationKind::Query => "query",
            MetricOperationKind::Mutation => "mutation",
            MetricOperationKind::Subscription => "subscription",
        };
        Self::static_value(MetricLabelKey::OperationKind, value)
    }

    /// Creates a label from an opaque registered-operation key using token characters.
    pub fn registered_operation_key(value: &str) -> Result<Self, MetricLabelError> {
        Self::token(MetricLabelKey::RegisteredOperationKey, value)
    }

    pub fn schema_digest(digest: SchemaDigest) -> Self {
        Self {
            key: MetricLabelKey::SchemaDigest,
            value: digest.to_hex(),
        }
    }

    pub fn lifecycle_phase(phase: LifecyclePhase) -> Self {
        Self::static_value(MetricLabelKey::LifecyclePhase, phase_name(phase))
    }

    /// Creates a result-code label from a framework-defined static value.
    pub fn result_code(value: &'static str) -> Result<Self, MetricLabelError> {
        Self::token(MetricLabelKey::ResultCode, value)
    }

    /// Creates an adapter label from a compile-time static name.
    pub fn adapter(value: &'static str) -> Result<Self, MetricLabelError> {
        Self::token(MetricLabelKey::Adapter, value)
    }

    /// Creates a provider label from a compile-time static name.
    pub fn provider(value: &'static str) -> Result<Self, MetricLabelError> {
        Self::token(MetricLabelKey::Provider, value)
    }

    fn static_value(key: MetricLabelKey, value: &'static str) -> Self {
        debug_assert!(value.len() <= LABEL_VALUE_BYTES);
        Self {
            key,
            value: value.to_owned(),
        }
    }

    fn token(key: MetricLabelKey, value: &str) -> Result<Self, MetricLabelError> {
        if value.is_empty()
            || value.len() > LABEL_VALUE_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
        {
            return Err(MetricLabelError);
        }
        Ok(Self {
            key,
            value: value.to_owned(),
        })
    }

    pub const fn key(&self) -> MetricLabelKey {
        self.key
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

fn phase_name(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::ServerBuilt => "server-built",
        LifecyclePhase::RequestStarted => "request-started",
        LifecyclePhase::HttpPreflight => "http-preflight",
        LifecyclePhase::TransportAdmission => "transport-admission",
        LifecyclePhase::BodyRead => "body-read",
        LifecyclePhase::HttpDecode => "http-decode",
        LifecyclePhase::OperationSourceResolution => "operation-source-resolution",
        LifecyclePhase::DecodedRequestHook => "decoded-request-hook",
        LifecyclePhase::ContextConstruction => "context-construction",
        LifecyclePhase::RequestPlugin => "request-plugin",
        LifecyclePhase::DocumentPreparation => "document-preparation",
        LifecyclePhase::OperationAdmission => "operation-admission",
        LifecyclePhase::Execution => "execution",
        LifecyclePhase::ResponseHook => "response-hook",
        LifecyclePhase::HttpEncode => "http-encode",
        LifecyclePhase::RequestCompleted => "request-completed",
    }
}

/// A validated collection of at most eight unique, bounded labels per metric.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetricLabels(Vec<MetricLabel>);

impl MetricLabels {
    pub fn new(labels: Vec<MetricLabel>) -> Result<Self, MetricLabelError> {
        if labels.len() > LABELS {
            return Err(MetricLabelError);
        }
        for (index, label) in labels.iter().enumerate() {
            if labels[..index]
                .iter()
                .any(|previous| previous.key == label.key)
            {
                return Err(MetricLabelError);
            }
        }
        Ok(Self(labels))
    }

    pub fn as_slice(&self) -> &[MetricLabel] {
        &self.0
    }
}

/// A metric label violated the key, value, count, or uniqueness contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetricLabelError;

impl fmt::Display for MetricLabelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("metric labels exceed the closed bounded contract")
    }
}

impl std::error::Error for MetricLabelError {}
