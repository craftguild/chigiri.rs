//! Samples queue occupancy for the same query through the real endpoint and worker,
//! without adding a measurement API to the public runtime.

use super::Worker;
use crate::hooks::HookSlot;
use crate::luau::{Contract, HookCandidate, ScriptSet};
use crate::service::HttpScope;
use crate::store::Store;
use chigiri::{BoxFuture, DeadlineFactory};
use chigiri_http::{ApplicationMetadata, BufferedBody, RequestHeader, TransportRequest};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SDL: &str = include_str!("../../../fixtures/hook_measure.graphql");
const QUERY: &str = "query HookCost { greeting notes { id title } }";
const SEED: &str =
    r#"{"Note":[{"id":"note-1","title":"First note"},{"id":"note-2","title":"Second note"}]}"#;

fn worker(enabled: bool) -> Worker {
    let ir = crate::service::tests::ir(SDL);
    let contract = Contract::new(ir.clone()).unwrap();
    let hooks = HookSlot::ALL
        .into_iter()
        .filter(|_| enabled)
        .map(|slot| HookCandidate {
            slot,
            source: crate::luau::render_hook_module(
                slot,
                "ctx.log('debug', 'hook-cost') return nil",
            )
            .unwrap(),
        })
        .collect();
    let hooks = contract.hooks(hooks, None).unwrap();
    let candidates = crate::service::tests::candidates(
        &ir,
        &[
            ("Query.greeting", "return 'hello'"),
            ("Query.notes", "return ctx.models.Note:find({})"),
        ],
    );
    let scripts = ScriptSet::new(contract, candidates).unwrap();
    Worker::from_checked(scripts, hooks, Store::from_seed_json(ir, SEED).unwrap()).unwrap()
}

struct Clock;
impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<(usize, usize)>>,
}
impl Sampler {
    fn start(client: &super::Client) -> Self {
        let sender = client.sender.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            let (mut maximum, mut samples) = (0, 0);
            while !done.load(Ordering::Relaxed) {
                maximum = maximum.max(sender.len());
                samples += 1;
                std::thread::sleep(Duration::from_micros(100));
            }
            (maximum, samples)
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    fn finish(mut self) -> (usize, usize) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn request(endpoint: &chigiri::Endpoint<super::Client>) -> serde_json::Value {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let response = scope
        .run(endpoint.handle(
            request,
            BufferedBody::new(serde_json::to_vec(&serde_json::json!({"query": QUERY})).unwrap()),
            &Clock,
        ))
        .await;
    serde_json::from_slice(&response.body).unwrap_or_else(|_| {
        assert_ne!(
            response.status, 200,
            "successful HTTP response contains invalid JSON"
        );
        serde_json::json!({"transport_status":response.status})
    })
}

#[tokio::test]
async fn hk_27_same_operation_samples_real_worker_queue_with_hooks_disabled_and_enabled() {
    let _case = crate::vm_gate::test_case();
    let expected = serde_json::json!({"data":{"greeting":"hello","notes":[
        {"id":"note-1","title":"First note"},{"id":"note-2","title":"Second note"}
    ]}});
    for enabled in [false, true] {
        let mut worker = worker(enabled);
        let endpoint = Arc::new(crate::service::tests::bound(worker.client.clone()));
        let sampler = Sampler::start(&worker.client);
        for _ in 0..10 {
            assert_eq!(request(&endpoint).await, expected);
        }
        let (serial_maximum, serial_samples) = sampler.finish();
        let sampler = Sampler::start(&worker.client);
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let endpoint = endpoint.clone();
            tasks.spawn(async move { request(&endpoint).await });
        }
        let mut successes = 0;
        while let Some(result) = tasks.join_next().await {
            successes += usize::from(result.unwrap() == expected);
        }
        let (parallel_maximum, parallel_samples) = sampler.finish();
        assert!(serial_maximum <= crate::limits::QUEUED_JOBS);
        assert!(parallel_maximum <= crate::limits::QUEUED_JOBS);
        assert_eq!(request(&endpoint).await, expected);
        worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        assert!(worker.thread.is_none());
        println!(
            "HOOK_QUEUE_MEASURE {}",
            serde_json::json!({
                "hooks_enabled":enabled,"query":QUERY,"queue_capacity":crate::limits::QUEUED_JOBS,
                "serial_requests":10,"serial_sampled_peak_jobs":serial_maximum,"serial_samples":serial_samples,
                "parallel_requests":8,"parallel_sampled_peak_jobs":parallel_maximum,"parallel_samples":parallel_samples,
                "parallel_successes":successes,"sample_sleep_microseconds":100,
                "note":"receiver length excludes executing job; scheduler adds sampling delay; sampled peak, not exact high-water mark"
            })
        );
    }
}
