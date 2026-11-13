//! Measures latency statistics and peak RSS in the same process.

use std::hint::black_box;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use chigiri::core::SourceId;
use chigiri::{ExecutionLimits, GraphQlRequest};
use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader, TransportRequest,
};

use super::fixture::{Fixture, ObserverSampleCounts};
use super::model::{BenchmarkRss, ObserverQueueEvidence};

pub(super) const QUERY: &str = "query Bench { books { id title author } }";
const BODY: &[u8] = b"{\"query\":\"query Bench { books { id title author } }\"}";
const HEADERS: &[RequestHeader<'_>] = &[
    RequestHeader::new("content-type", b"application/json"),
    RequestHeader::new("accept", b"application/graphql-response+json"),
];

pub(super) fn sync_samples<T>(
    samples: usize,
    iterations: usize,
    mut operation: impl FnMut() -> T,
) -> Vec<f64> {
    (0..samples)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..iterations {
                black_box(operation());
            }
            started.elapsed().as_nanos() as f64 / iterations as f64
        })
        .collect()
}

pub(super) fn parse_samples(samples: usize, iterations: usize) -> Vec<f64> {
    sync_samples(samples, iterations, || {
        chigiri::core::language::parse_executable_document(
            SourceId::new(81),
            black_box(QUERY),
            &ExecutionLimits::production(),
        )
    })
}

pub(super) fn validate_samples(fixture: &Fixture, samples: usize, iterations: usize) -> Vec<f64> {
    let request = GraphQlRequest::new(QUERY);
    sync_samples(samples, iterations, || {
        fixture.endpoint.validate_operation(black_box(&request))
    })
}

pub(super) async fn execute_samples(
    fixture: &Fixture,
    samples: usize,
    iterations: usize,
) -> Vec<f64> {
    let request = GraphQlRequest::new(QUERY);
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(
                fixture
                    .endpoint
                    .execute(request.clone(), fixture.context.clone(), &TokioDeadline)
                    .await,
            );
        }
        values.push(started.elapsed().as_nanos() as f64 / iterations as f64);
    }
    values
}

pub(super) async fn endpoint_profile_samples(
    fixtures: &[&Fixture],
    samples: usize,
    iterations: usize,
) -> Result<EndpointProfileSamples, Box<dyn std::error::Error>> {
    for fixture in fixtures {
        fixture.prepare_observer_measurement()?;
    }
    let mut values = vec![Vec::with_capacity(samples); fixtures.len()];
    let mut queue_counts = vec![ObserverSampleCounts::default(); fixtures.len()];
    for sample in 0..samples {
        if sample % 2 == 0 {
            for index in 0..fixtures.len() {
                record_endpoint_sample(
                    fixtures[index],
                    iterations,
                    &mut values[index],
                    &mut queue_counts[index],
                )
                .await?;
            }
        } else {
            for index in (0..fixtures.len()).rev() {
                record_endpoint_sample(
                    fixtures[index],
                    iterations,
                    &mut values[index],
                    &mut queue_counts[index],
                )
                .await?;
            }
        }
    }
    let observer_queues = fixtures
        .iter()
        .zip(queue_counts)
        .filter_map(|(fixture, counts)| {
            fixture
                .observer_queue_evidence(samples, iterations, counts)
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EndpointProfileSamples {
        values,
        observer_queues,
    })
}

pub(super) struct EndpointProfileSamples {
    pub(super) values: Vec<Vec<f64>>,
    pub(super) observer_queues: Vec<ObserverQueueEvidence>,
}

async fn record_endpoint_sample(
    fixture: &Fixture,
    iterations: usize,
    values: &mut Vec<f64>,
    queue_counts: &mut ObserverSampleCounts,
) -> Result<(), Box<dyn std::error::Error>> {
    values.push(endpoint_sample(fixture, iterations).await);
    if let Some(counts) = fixture.drain_observer_sample(iterations)? {
        queue_counts.add(counts);
    }
    Ok(())
}

async fn endpoint_sample(fixture: &Fixture, iterations: usize) -> f64 {
    let metadata = ApplicationMetadata::default();
    let deadlines = TokioDeadline;
    let started = Instant::now();
    for _ in 0..iterations {
        let request = TransportRequest::new("POST", "/graphql", None, HEADERS, None, &metadata);
        black_box(
            fixture
                .endpoint
                .handle(request, BufferedBody::new(BODY.to_vec()), &deadlines)
                .await,
        );
    }
    started.elapsed().as_nanos() as f64 / iterations as f64
}

pub(super) fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

pub(super) struct RssMonitor {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    attempts: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RssMonitor {
    pub(super) fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let attempts = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        let errors = Arc::new(AtomicU64::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_peak = Arc::clone(&peak);
        let thread_attempts = Arc::clone(&attempts);
        let thread_samples = Arc::clone(&samples);
        let thread_errors = Arc::clone(&errors);
        let pid = std::process::id().to_string();
        let thread = thread::spawn(move || {
            loop {
                thread_attempts.fetch_add(1, Ordering::AcqRel);
                match read_rss(&pid) {
                    Ok(rss) => {
                        thread_samples.fetch_add(1, Ordering::AcqRel);
                        thread_peak.fetch_max(rss, Ordering::AcqRel);
                    }
                    Err(()) => {
                        thread_errors.fetch_add(1, Ordering::AcqRel);
                    }
                }
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            peak,
            attempts,
            samples,
            errors,
            thread: Some(thread),
        }
    }

    pub(super) fn finish(mut self) -> Result<BenchmarkRss, Box<dyn std::error::Error>> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "The RSS sampler thread panicked.")?;
        }
        let attempts = self.attempts.load(Ordering::Acquire);
        let samples = self.samples.load(Ordering::Acquire);
        let errors = self.errors.load(Ordering::Acquire);
        if attempts == 0 || samples == 0 || errors != 0 || samples != attempts {
            return Err("The RSS sampler could not get an active sample from all attempts.".into());
        }
        Ok(BenchmarkRss {
            peak_rss_bytes: self.peak.load(Ordering::Acquire),
            sample_count: samples,
            expected_sample_count: attempts,
            coverage: samples as f64 / attempts as f64,
            error_count: errors,
            thread_stopped: true,
        })
    }
}

fn read_rss(pid: &str) -> Result<u64, ()> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", pid])
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let kib = String::from_utf8(output.stdout)
        .map_err(|_| ())?
        .trim()
        .parse::<u64>()
        .map_err(|_| ())?;
    kib.checked_mul(1024).ok_or(())
}

pub(super) struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
