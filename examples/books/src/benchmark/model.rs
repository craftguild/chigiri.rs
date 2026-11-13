//! Closed evidence model for benchmark-baseline comparison.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Hardware {
    pub(super) os: String,
    pub(super) os_version: String,
    pub(super) arch: String,
    pub(super) logical_cpus: usize,
    pub(super) descriptor: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Toolchain {
    pub(super) version: String,
    pub(super) cargo: String,
    pub(super) rustc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SourceIdentity {
    pub(super) revision: String,
    pub(super) revision_unix_seconds: u64,
    pub(super) dirty: bool,
    pub(super) binary_sha256: String,
    pub(super) toolchain: Toolchain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProfileDefinition {
    pub(super) name: String,
    pub(super) includes: Vec<String>,
    pub(super) excludes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Measurement {
    pub(super) name: String,
    pub(super) median_ns: f64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserverQueueEvidence {
    pub(super) name: String,
    pub(super) capacity: usize,
    pub(super) lifecycle_events_per_request: usize,
    pub(super) expected_lifecycle_event_count: u64,
    pub(super) accepted_lifecycle_event_count: u64,
    pub(super) dropped_lifecycle_event_count: u64,
    pub(super) accepted_error_event_count: u64,
    pub(super) dropped_error_event_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BenchmarkRss {
    pub(super) peak_rss_bytes: u64,
    pub(super) sample_count: u64,
    pub(super) expected_sample_count: u64,
    pub(super) coverage: f64,
    pub(super) error_count: u64,
    pub(super) thread_stopped: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Baseline {
    pub(super) format_version: u32,
    pub(super) accepted: bool,
    pub(super) accepted_by: Option<String>,
    pub(super) accepted_at: Option<String>,
    pub(super) accepted_at_unix_seconds: Option<u64>,
    pub(super) accepted_reason: Option<String>,
    pub(super) recorded_at_unix_seconds: u64,
    pub(super) command: Vec<String>,
    pub(super) source: SourceIdentity,
    pub(super) hardware: Hardware,
    pub(super) samples: usize,
    pub(super) iterations_per_sample: usize,
    pub(super) rss: BenchmarkRss,
    pub(super) profile_order_strategy: String,
    pub(super) profiles: Vec<ProfileDefinition>,
    pub(super) observer_queues: Vec<ObserverQueueEvidence>,
    pub(super) diagnostic_profiles: Vec<ProfileDefinition>,
    pub(super) diagnostic_measurements: Vec<Measurement>,
    pub(super) diagnostic_observer_queues: Vec<ObserverQueueEvidence>,
    pub(super) measurements: Vec<Measurement>,
    pub(super) overheads: Vec<Overhead>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metric {
    pub(super) name: String,
    pub(super) baseline_median_ns: f64,
    pub(super) candidate_median_ns: f64,
    pub(super) regression_percent: f64,
    pub(super) limit_percent: f64,
    pub(super) passed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Overhead {
    pub(super) name: String,
    pub(super) disabled_median_ns: f64,
    pub(super) enabled_median_ns: f64,
    pub(super) overhead_percent: f64,
    pub(super) limit_percent: f64,
    pub(super) passed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Evidence {
    pub(super) format_version: u32,
    pub(super) passed: bool,
    pub(super) recorded_at_unix_seconds: u64,
    pub(super) baseline_path: String,
    pub(super) comparison_mode: String,
    pub(super) baseline_source: SourceIdentity,
    pub(super) source: SourceIdentity,
    pub(super) hardware: Hardware,
    pub(super) command: Vec<String>,
    pub(super) samples: usize,
    pub(super) iterations_per_sample: usize,
    pub(super) rss: BenchmarkRss,
    pub(super) profile_order_strategy: String,
    pub(super) profiles: Vec<ProfileDefinition>,
    pub(super) observer_queues: Vec<ObserverQueueEvidence>,
    pub(super) diagnostic_profiles: Vec<ProfileDefinition>,
    pub(super) diagnostic_measurements: Vec<Measurement>,
    pub(super) diagnostic_observer_queues: Vec<ObserverQueueEvidence>,
    pub(super) metrics: Vec<Metric>,
    pub(super) overheads: Vec<Overhead>,
}

#[cfg(test)]
mod tests {
    use super::Hardware;

    #[test]
    fn hardware_rejects_unknown_fields_and_invalid_integer_values() {
        let valid = r#"{
            "os":"macos","osVersion":"26.4","arch":"aarch64",
            "logicalCpus":10,"descriptor":"Apple M4"
        }"#;
        assert!(serde_json::from_str::<Hardware>(valid).is_ok());
        for invalid in [
            valid.replace("\n        }", ",\"unknown\":true\n        }"),
            valid.replace("\"logicalCpus\":10", "\"logicalCpus\":10.0"),
            valid.replace("\"logicalCpus\":10", "\"logicalCpus\":false"),
        ] {
            assert!(serde_json::from_str::<Hardware>(&invalid).is_err());
        }
    }
}
