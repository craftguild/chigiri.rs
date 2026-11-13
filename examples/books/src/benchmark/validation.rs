//! Validates benchmark source mode and numerical evidence independently of self-declared status.

use super::fixture::{DiagnosticProfile, LIFECYCLE_EVENTS_PER_REQUEST, OBSERVER_CAPACITY, Profile};
use super::model::{Baseline, BenchmarkRss, Measurement, Metric, ObserverQueueEvidence, Overhead};
use super::settings::{ComparisonMode, Mode, Settings};
use super::{CandidateRun, FORMAT_VERSION, PROFILE_ORDER_STRATEGY, identity, timestamp};

pub(super) fn validate_candidate(
    candidate: &CandidateRun,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_audit_metadata(
        candidate.recorded_at_unix_seconds,
        candidate.source.revision_unix_seconds,
        &candidate.hardware,
    )?;
    validate_measurements(&candidate.measurements)?;
    validate_observer_queues(
        &candidate.observer_queues,
        candidate.samples,
        candidate.iterations_per_sample,
        &[Profile::Observer.name(), Profile::Facade.name()],
    )?;
    validate_diagnostic_measurements(&candidate.diagnostic_measurements)?;
    validate_observer_queues(
        &candidate.diagnostic_observer_queues,
        candidate.samples,
        candidate.iterations_per_sample,
        &[
            DiagnosticProfile::QueueOnly.name(),
            DiagnosticProfile::FullObserver.name(),
        ],
    )?;
    validate_rss(&candidate.rss)?;
    let disabled = disabled_median(&candidate.measurements)?;
    validate_overheads(&candidate.overheads, disabled, false)?;
    Ok(())
}

pub(super) fn validate_baseline(
    baseline: &Baseline,
    candidate: &CandidateRun,
    mode: &ComparisonMode,
) -> Result<(), Box<dyn std::error::Error>> {
    if baseline.format_version != FORMAT_VERSION {
        return Err(
            "The baseline format version does not match the expected format version.".into(),
        );
    }
    validate_approval(baseline)?;
    validate_audit_metadata(
        baseline.recorded_at_unix_seconds,
        baseline.source.revision_unix_seconds,
        &baseline.hardware,
    )?;
    let command = Settings::parse(baseline.command.clone())?;
    if !matches!(command.mode, Mode::Record)
        || command.toolchain != baseline.source.toolchain.version
        || command.samples != baseline.samples
        || command.iterations != baseline.iterations_per_sample
    {
        return Err("The baseline command cannot be reproduced from its audit fields.".into());
    }
    validate_measurements(&baseline.measurements)?;
    validate_observer_queues(
        &baseline.observer_queues,
        baseline.samples,
        baseline.iterations_per_sample,
        &[Profile::Observer.name(), Profile::Facade.name()],
    )?;
    validate_diagnostic_measurements(&baseline.diagnostic_measurements)?;
    validate_observer_queues(
        &baseline.diagnostic_observer_queues,
        baseline.samples,
        baseline.iterations_per_sample,
        &[
            DiagnosticProfile::QueueOnly.name(),
            DiagnosticProfile::FullObserver.name(),
        ],
    )?;
    validate_rss(&baseline.rss)?;
    let disabled = disabled_median(&baseline.measurements)?;
    validate_overheads(&baseline.overheads, disabled, true)?;
    validate_source(baseline, candidate, mode)?;
    if baseline.hardware != candidate.hardware {
        return Err("The baseline and the candidate hardware don't match.".into());
    }
    if baseline.samples != candidate.samples
        || baseline.iterations_per_sample != candidate.iterations_per_sample
    {
        return Err(
            "The baseline and the candidate measurement conditions are not the same.".into(),
        );
    }
    if baseline.profile_order_strategy != PROFILE_ORDER_STRATEGY
        || baseline.profiles != candidate.profiles
        || baseline.observer_queues != candidate.observer_queues
        || baseline.diagnostic_profiles != candidate.diagnostic_profiles
        || baseline.diagnostic_observer_queues != candidate.diagnostic_observer_queues
    {
        return Err("Baseline and candidate profile contracts do not match.".into());
    }
    Ok(())
}

pub(super) fn validate_observer_queues(
    values: &[ObserverQueueEvidence],
    samples: usize,
    iterations: usize,
    expected_names: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = samples
        .checked_mul(iterations)
        .and_then(|value| value.checked_mul(LIFECYCLE_EVENTS_PER_REQUEST))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or("Expected observer-evidence count overflowed.")?;
    if values.len() != expected_names.len()
        || values.iter().zip(expected_names).any(|(value, name)| {
            value.name != *name
                || value.capacity != OBSERVER_CAPACITY
                || value.lifecycle_events_per_request != LIFECYCLE_EVENTS_PER_REQUEST
                || value.expected_lifecycle_event_count != expected
                || value.accepted_lifecycle_event_count != expected
                || value.dropped_lifecycle_event_count != 0
                || value.accepted_error_event_count != 0
                || value.dropped_error_event_count != 0
        })
    {
        return Err("Observer-queue evidence does not show delivery of every event.".into());
    }
    Ok(())
}

fn validate_diagnostic_measurements(
    values: &[Measurement],
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = DiagnosticProfile::ALL.map(DiagnosticProfile::name);
    if values.len() != expected.len()
        || values.iter().zip(expected).any(|(value, name)| {
            value.name != name || !value.median_ns.is_finite() || value.median_ns <= 0.0
        })
    {
        return Err("Diagnostic-profile medians do not match the closed set.".into());
    }
    Ok(())
}

pub(super) fn validate_audit_metadata(
    recorded_at: u64,
    revision_at: u64,
    hardware: &super::model::Hardware,
) -> Result<(), Box<dyn std::error::Error>> {
    const MIN_RECORDED: u64 = 1_577_836_800;
    let now = identity::recorded_at_unix_seconds()?;
    if recorded_at < MIN_RECORDED
        || recorded_at < revision_at
        || recorded_at > now.saturating_add(300)
    {
        return Err("The benchmark recording time is outside the auditable range.".into());
    }
    if hardware.os.trim().is_empty()
        || hardware.os_version.trim().is_empty()
        || hardware.arch.trim().is_empty()
        || hardware.descriptor.trim().is_empty()
        || hardware.logical_cpus == 0
    {
        return Err("The benchmark hardware metadata is incomplete.".into());
    }
    Ok(())
}

fn validate_source(
    baseline: &Baseline,
    candidate: &CandidateRun,
    mode: &ComparisonMode,
) -> Result<(), Box<dyn std::error::Error>> {
    if identity::revision_unix_seconds(&baseline.source.revision)?
        != baseline.source.revision_unix_seconds
    {
        return Err("The baseline source revision timestamp doesn't match the Git history.".into());
    }
    if baseline.source.dirty || baseline.source.toolchain != candidate.source.toolchain {
        return Err("baseline does not match the clean source or toolchain".into());
    }
    match mode {
        ComparisonMode::InitialCandidate if baseline.source != candidate.source => {
            Err("The first baseline doesn't match the candidate's revision/binary.".into())
        }
        ComparisonMode::PreviousRelease(binary) => {
            if baseline.source.revision == candidate.source.revision {
                return Err(
                    "The pre-release baseline needs a different revision than the candidate."
                        .into(),
                );
            }
            if identity::binary_sha256(binary)? != baseline.source.binary_sha256 {
                return Err(
                    "The digest of the pre-release benchmark binary does not match.".into(),
                );
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_approval(baseline: &Baseline) -> Result<(), Box<dyn std::error::Error>> {
    let reviewer = baseline.accepted_by.as_deref().unwrap_or_default().trim();
    let reason = baseline
        .accepted_reason
        .as_deref()
        .unwrap_or_default()
        .trim();
    let generic = ["release-owner", "owner", "reviewer", "unknown"];
    let accepted_at = baseline.accepted_at.as_deref().unwrap_or_default();
    let accepted_unix = baseline.accepted_at_unix_seconds.unwrap_or_default();
    let parsed_accepted_unix = timestamp::parse_canonical_utc_seconds(accepted_at);
    if !baseline.accepted
        || reviewer.is_empty()
        || generic.contains(&reviewer.to_ascii_lowercase().as_str())
        || parsed_accepted_unix != Some(accepted_unix)
        || accepted_unix < baseline.recorded_at_unix_seconds
        || accepted_unix > baseline.recorded_at_unix_seconds.saturating_add(86_400)
        || reason.chars().count() < 40
    {
        return Err("The baseline has no auditable acceptance record.".into());
    }
    Ok(())
}

pub(super) fn validate_measurements(
    values: &[Measurement],
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = [
        "parse",
        "validate",
        "cache-hit",
        "execute",
        "policy-disabled",
    ];
    if values.len() != expected.len()
        || values.iter().zip(expected).any(|(value, name)| {
            value.name != name || !value.median_ns.is_finite() || value.median_ns <= 0.0
        })
    {
        return Err(
            "Benchmark medians must be finite positive numbers for exactly the required metric set."
                .into(),
        );
    }
    Ok(())
}

pub(super) fn validate_rss(rss: &BenchmarkRss) -> Result<(), Box<dyn std::error::Error>> {
    let calculated = rss.sample_count as f64 / rss.expected_sample_count as f64;
    if rss.peak_rss_bytes == 0
        || rss.sample_count == 0
        || rss.sample_count != rss.expected_sample_count
        || rss.error_count != 0
        || !rss.thread_stopped
        || !rss.coverage.is_finite()
        || !close(rss.coverage, calculated)
    {
        return Err("RSS evidence does not show that every attempt succeeded.".into());
    }
    Ok(())
}

pub(super) fn validate_overheads(
    values: &[Overhead],
    disabled: f64,
    require_pass: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let profiles = &Profile::ALL[1..];
    if values.len() != profiles.len() {
        return Err("Enabled-profile evidence count does not match.".into());
    }
    for (value, profile) in values.iter().zip(profiles) {
        let limit = if *profile == Profile::Facade {
            50.0
        } else {
            25.0
        };
        let calculated = percent(value.enabled_median_ns, value.disabled_median_ns);
        let passed = calculated <= limit;
        if value.name != profile.name()
            || !value.disabled_median_ns.is_finite()
            || !value.enabled_median_ns.is_finite()
            || value.disabled_median_ns <= 0.0
            || value.enabled_median_ns <= 0.0
            || !close(value.disabled_median_ns, disabled)
            || value.limit_percent != limit
            || !value.overhead_percent.is_finite()
            || !close(value.overhead_percent, calculated)
            || value.passed != passed
            || (require_pass && !passed)
        {
            return Err("The enabled-profile overhead cannot be derived from the median.".into());
        }
    }
    Ok(())
}

pub(super) fn compare_measurements(
    baseline: &Baseline,
    measurements: Vec<Measurement>,
) -> Result<Vec<Metric>, Box<dyn std::error::Error>> {
    measurements
        .into_iter()
        .zip(&baseline.measurements)
        .map(|(candidate, baseline)| {
            if candidate.name != baseline.name {
                return Err("Baseline metric order does not match.".into());
            }
            let limit = if candidate.name == Profile::Disabled.name() {
                5.0
            } else {
                15.0
            };
            let regression = percent(candidate.median_ns, baseline.median_ns);
            Ok(Metric {
                name: candidate.name,
                baseline_median_ns: baseline.median_ns,
                candidate_median_ns: candidate.median_ns,
                regression_percent: regression,
                limit_percent: limit,
                passed: regression <= limit,
            })
        })
        .collect()
}

fn disabled_median(values: &[Measurement]) -> Result<f64, Box<dyn std::error::Error>> {
    values
        .iter()
        .find(|value| value.name == Profile::Disabled.name())
        .map(|value| value.median_ns)
        .ok_or_else(|| "The policy-disabled median is missing.".into())
}

pub(super) fn percent(candidate: f64, baseline: f64) -> f64 {
    (candidate / baseline - 1.0) * 100.0
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9_f64.max(right.abs() * 1e-12)
}
