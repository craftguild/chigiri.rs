//! The accepted baseline is required to generate relative performance evidence with source attribution.

mod cli;
mod fixture;
#[cfg(test)]
mod fixture_tests;
mod identity;
mod measure;
mod model;
mod settings;
mod timestamp;
mod validation;
#[cfg(test)]
mod validation_tests;

use std::path::Path;

use fixture::{
    DiagnosticProfile, Fixture, Profile, diagnostic_profile_definitions, profile_definitions,
};
use measure::{RssMonitor, median};
use model::{
    Baseline, BenchmarkRss, Evidence, Hardware, Measurement, ObserverQueueEvidence, Overhead,
    ProfileDefinition, SourceIdentity,
};
use settings::{Mode, Settings};
use validation::{compare_measurements, validate_baseline, validate_candidate};

const FORMAT_VERSION: u32 = 5;
const PROFILE_ORDER_STRATEGY: &str = "alternating-forward-reverse";

/// Parse CLI arguments and start measurement. Help and version output have no side effects.
pub fn main_entry(arguments: Vec<std::ffi::OsString>) -> i32 {
    cli::main_entry(arguments)
}

async fn run(settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    let samples = settings.samples;
    let iterations = settings.iterations;
    if samples < 5 || iterations == 0 {
        return Err("Benchmarking requires at least five samples and one iteration.".into());
    }
    let source = identity::source_identity(&settings.toolchain)?;
    if source.dirty {
        return Err("run the benchmark from a clean detached HEAD".into());
    }
    let hardware = identity::hardware()?;
    let profiles = profile_definitions();
    let diagnostic_profiles = diagnostic_profile_definitions();
    let measured = measure(samples, iterations).await?;
    let candidate = CandidateRun {
        source,
        hardware,
        recorded_at_unix_seconds: identity::recorded_at_unix_seconds()?,
        command: settings.command,
        samples,
        iterations_per_sample: iterations,
        rss: measured.rss,
        profiles,
        observer_queues: measured.observer_queues,
        diagnostic_profiles,
        diagnostic_measurements: measured.diagnostic_measurements,
        diagnostic_observer_queues: measured.diagnostic_observer_queues,
        measurements: measured.measurements,
        overheads: measured.overheads,
    };
    validate_candidate(&candidate)?;
    match settings.mode {
        Mode::Record => write_record(candidate, &settings.output),
        Mode::Compare {
            baseline,
            comparison,
        } => compare(candidate, &baseline, &comparison, &settings.output),
    }
}

struct CandidateRun {
    source: SourceIdentity,
    hardware: Hardware,
    recorded_at_unix_seconds: u64,
    command: Vec<String>,
    samples: usize,
    iterations_per_sample: usize,
    rss: BenchmarkRss,
    profiles: Vec<ProfileDefinition>,
    observer_queues: Vec<ObserverQueueEvidence>,
    diagnostic_profiles: Vec<ProfileDefinition>,
    diagnostic_measurements: Vec<Measurement>,
    diagnostic_observer_queues: Vec<ObserverQueueEvidence>,
    measurements: Vec<Measurement>,
    overheads: Vec<Overhead>,
}

struct BenchmarkMeasurements {
    measurements: Vec<Measurement>,
    overheads: Vec<Overhead>,
    observer_queues: Vec<ObserverQueueEvidence>,
    diagnostic_measurements: Vec<Measurement>,
    diagnostic_observer_queues: Vec<ObserverQueueEvidence>,
    rss: BenchmarkRss,
}

async fn measure(
    samples: usize,
    iterations: usize,
) -> Result<BenchmarkMeasurements, Box<dyn std::error::Error>> {
    let rss = RssMonitor::start();
    let validation = Fixture::new(Profile::Disabled, false)?;
    let fixtures = Profile::ALL
        .into_iter()
        .map(|profile| Fixture::new(profile, true))
        .collect::<Result<Vec<_>, _>>()?;
    let diagnostic_fixtures = DiagnosticProfile::ALL
        .into_iter()
        .map(Fixture::new_diagnostic)
        .collect::<Result<Vec<_>, _>>()?;
    let cached = &fixtures[0];
    let _ = cached
        .endpoint
        .validate_operation(&chigiri::GraphQlRequest::new(measure::QUERY));
    let _ = cached
        .endpoint
        .execute(
            chigiri::GraphQlRequest::new(measure::QUERY),
            cached.context.clone(),
            &measure::TokioDeadline,
        )
        .await;
    let references = fixtures.iter().collect::<Vec<_>>();
    let measured = measure::endpoint_profile_samples(&references, samples, iterations).await?;
    let mut profile_samples = measured.values;
    let diagnostic_references = diagnostic_fixtures.iter().collect::<Vec<_>>();
    let diagnostic =
        measure::endpoint_profile_samples(&diagnostic_references, samples, iterations).await?;
    let disabled = median_value(std::mem::take(&mut profile_samples[0]));
    let measurements = vec![
        measurement("parse", measure::parse_samples(samples, iterations)),
        measurement(
            "validate",
            measure::validate_samples(&validation, samples, iterations),
        ),
        measurement(
            "cache-hit",
            measure::validate_samples(cached, samples, iterations),
        ),
        measurement(
            "execute",
            measure::execute_samples(cached, samples, iterations).await,
        ),
        Measurement {
            name: Profile::Disabled.name().to_owned(),
            median_ns: disabled,
        },
    ];
    let overheads = Profile::ALL[1..]
        .iter()
        .enumerate()
        .map(|(offset, profile)| {
            let limit = if *profile == Profile::Facade {
                50.0
            } else {
                25.0
            };
            enabled_overhead(
                profile.name(),
                disabled,
                std::mem::take(&mut profile_samples[offset + 1]),
                limit,
            )
        })
        .collect();
    let diagnostic_measurements = DiagnosticProfile::ALL
        .into_iter()
        .zip(diagnostic.values)
        .map(|(profile, values)| measurement(profile.name(), values))
        .collect();
    Ok(BenchmarkMeasurements {
        measurements,
        overheads,
        observer_queues: measured.observer_queues,
        diagnostic_measurements,
        diagnostic_observer_queues: diagnostic.observer_queues,
        rss: rss.finish()?,
    })
}

fn write_record(candidate: CandidateRun, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let baseline = Baseline {
        format_version: FORMAT_VERSION,
        accepted: false,
        accepted_by: None,
        accepted_at: None,
        accepted_at_unix_seconds: None,
        accepted_reason: None,
        recorded_at_unix_seconds: candidate.recorded_at_unix_seconds,
        command: candidate.command,
        source: candidate.source,
        hardware: candidate.hardware,
        samples: candidate.samples,
        iterations_per_sample: candidate.iterations_per_sample,
        rss: candidate.rss,
        profile_order_strategy: PROFILE_ORDER_STRATEGY.to_owned(),
        profiles: candidate.profiles,
        observer_queues: candidate.observer_queues,
        diagnostic_profiles: candidate.diagnostic_profiles,
        diagnostic_measurements: candidate.diagnostic_measurements,
        diagnostic_observer_queues: candidate.diagnostic_observer_queues,
        measurements: candidate.measurements,
        overheads: candidate.overheads,
    };
    write_json(output, &baseline)
}

fn compare(
    candidate: CandidateRun,
    path: &Path,
    comparison_mode: &settings::ComparisonMode,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline: Baseline = serde_json::from_slice(&std::fs::read(path)?)?;
    validate_baseline(&baseline, &candidate, comparison_mode)?;
    let metrics = compare_measurements(&baseline, candidate.measurements)?;
    let passed = metrics.iter().all(|metric| metric.passed)
        && candidate.overheads.iter().all(|overhead| overhead.passed);
    let evidence = Evidence {
        format_version: FORMAT_VERSION,
        passed,
        recorded_at_unix_seconds: candidate.recorded_at_unix_seconds,
        baseline_path: path.display().to_string(),
        comparison_mode: comparison_mode.name().to_owned(),
        baseline_source: baseline.source.clone(),
        source: candidate.source,
        hardware: candidate.hardware,
        command: candidate.command,
        samples: candidate.samples,
        iterations_per_sample: candidate.iterations_per_sample,
        rss: candidate.rss,
        profile_order_strategy: PROFILE_ORDER_STRATEGY.to_owned(),
        profiles: candidate.profiles,
        observer_queues: candidate.observer_queues,
        diagnostic_profiles: candidate.diagnostic_profiles,
        diagnostic_measurements: candidate.diagnostic_measurements,
        diagnostic_observer_queues: candidate.diagnostic_observer_queues,
        metrics,
        overheads: candidate.overheads,
    };
    write_json(output, &evidence)?;
    if passed {
        Ok(())
    } else {
        Err("benchmark exceeded the performance threshold".into())
    }
}

fn measurement(name: &str, values: Vec<f64>) -> Measurement {
    Measurement {
        name: name.to_owned(),
        median_ns: median_value(values),
    }
}

fn enabled_overhead(name: &str, disabled: f64, values: Vec<f64>, limit: f64) -> Overhead {
    let enabled = median_value(values);
    let overhead = validation::percent(enabled, disabled);
    Overhead {
        name: name.to_owned(),
        disabled_median_ns: disabled,
        enabled_median_ns: enabled,
        overhead_percent: overhead,
        limit_percent: limit,
        passed: overhead <= limit,
    }
}

fn median_value(mut values: Vec<f64>) -> f64 {
    median(&mut values)
}

fn write_json(
    path: &Path,
    value: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(path, &json)?;
    print!("{json}");
    Ok(())
}
