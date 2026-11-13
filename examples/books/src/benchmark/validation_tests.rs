//! Tests rejection paths in the benchmark validator.

use super::fixture::{LIFECYCLE_EVENTS_PER_REQUEST, OBSERVER_CAPACITY, Profile};
use super::model::{BenchmarkRss, Hardware, Measurement, ObserverQueueEvidence, Overhead};
use super::validation::{
    validate_audit_metadata, validate_measurements, validate_observer_queues, validate_overheads,
    validate_rss,
};

#[test]
fn rejects_negative_medians_and_inconsistent_overhead_claims() {
    let names = [
        "parse",
        "validate",
        "cache-hit",
        "execute",
        "policy-disabled",
    ];
    let mut measurements = names
        .into_iter()
        .map(|name| Measurement {
            name: name.to_owned(),
            median_ns: 1.0,
        })
        .collect::<Vec<_>>();
    measurements[0].median_ns = -1.0;
    assert!(validate_measurements(&measurements).is_err());

    let names = [
        "operation-enabled",
        "rate-enabled",
        "observer-enabled",
        "facade-composition-enabled",
    ];
    let overheads = names
        .into_iter()
        .enumerate()
        .map(|(index, name)| Overhead {
            name: name.to_owned(),
            disabled_median_ns: 1.0,
            enabled_median_ns: if index == 0 { 2.0 } else { 1.0 },
            overhead_percent: 0.0,
            limit_percent: if index == 3 { 50.0 } else { 25.0 },
            passed: true,
        })
        .collect::<Vec<_>>();
    assert!(validate_overheads(&overheads, 1.0, true).is_err());
}

#[test]
fn rejects_rss_evidence_without_successful_samples() {
    let rss = BenchmarkRss {
        peak_rss_bytes: 0,
        sample_count: 0,
        expected_sample_count: 1,
        coverage: 0.0,
        error_count: 1,
        thread_stopped: true,
    };
    assert!(validate_rss(&rss).is_err());
}

#[test]
fn requires_the_recording_time_to_follow_the_source_revision() {
    let hardware = Hardware {
        os: "macos".to_owned(),
        os_version: "26.4".to_owned(),
        arch: "aarch64".to_owned(),
        logical_cpus: 10,
        descriptor: "Apple M4".to_owned(),
    };
    let now = super::identity::recorded_at_unix_seconds().unwrap();
    assert!(validate_audit_metadata(now, now + 1, &hardware).is_err());
}

#[test]
fn rejects_dropped_or_missing_observer_queue_events() {
    let expected = u64::try_from(LIFECYCLE_EVENTS_PER_REQUEST).unwrap();
    let mut queue = ObserverQueueEvidence {
        name: Profile::Observer.name().to_owned(),
        capacity: OBSERVER_CAPACITY,
        lifecycle_events_per_request: LIFECYCLE_EVENTS_PER_REQUEST,
        expected_lifecycle_event_count: expected,
        accepted_lifecycle_event_count: expected,
        dropped_lifecycle_event_count: 0,
        accepted_error_event_count: 0,
        dropped_error_event_count: 0,
    };
    let names = [Profile::Observer.name()];
    assert!(validate_observer_queues(&[queue.clone()], 1, 1, &names).is_ok());
    queue.dropped_lifecycle_event_count = 1;
    assert!(validate_observer_queues(&[queue.clone()], 1, 1, &names).is_err());
    queue.dropped_lifecycle_event_count = 0;
    queue.accepted_lifecycle_event_count -= 1;
    assert!(validate_observer_queues(&[queue], 1, 1, &names).is_err());
}
