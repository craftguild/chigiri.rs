//! Verify that the request ID initialization and parallel generation are worker-local and non-linear.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::Duration;

use chigiri::{RequestId, RequestIdFactory, RequestIdFactoryError};

use super::support::resolvers;

#[derive(Clone)]
struct CountingFactory {
    calls: Arc<AtomicUsize>,
}

impl RequestIdFactory for CountingFactory {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        request_id(self.calls.fetch_add(1, Ordering::SeqCst))
    }
}

#[test]
fn request_id_factory_is_initialized_at_build_and_called_once_for_the_first_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let builder = chigiri::Endpoint::builder(resolvers()).inject(|components| {
        components
            .observability()
            .request_id_factory(CountingFactory {
                calls: Arc::clone(&calls),
            });
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let endpoint = builder.without_context().build().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        endpoint
            .request_telemetry(None)
            .request_id()
            .unwrap()
            .as_str(),
        "request-0000000000000000"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        endpoint
            .request_telemetry(None)
            .request_id()
            .unwrap()
            .as_str(),
        "request-0000000000000001"
    );
}

#[test]
fn request_ids_are_unique_during_initial_parallel_generation() {
    const PRODUCERS: usize = 32;

    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .observability()
                .request_id_factory(CountingFactory {
                    calls: Arc::clone(&calls),
                });
        })
        .without_context()
        .build()
        .unwrap();
    let start = Arc::new(Barrier::new(PRODUCERS));
    let handles = (0..PRODUCERS)
        .map(|_| {
            let endpoint = endpoint.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                endpoint
                    .request_telemetry(None)
                    .request_id()
                    .unwrap()
                    .as_str()
                    .to_owned()
            })
        })
        .collect::<Vec<_>>();
    let values = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.iter().collect::<HashSet<_>>().len(), PRODUCERS);
    assert_eq!(calls.load(Ordering::SeqCst), PRODUCERS);
}

#[derive(Clone)]
struct BlockingFactory {
    calls: Arc<AtomicUsize>,
    release: Arc<(Mutex<bool>, Condvar)>,
    timed_out: Arc<AtomicBool>,
}

impl RequestIdFactory for BlockingFactory {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        match call {
            1 => {
                let (lock, ready) = &*self.release;
                let released = lock.lock().unwrap();
                let (_released, result) = ready
                    .wait_timeout_while(released, Duration::from_secs(1), |value| !*value)
                    .unwrap();
                self.timed_out.store(result.timed_out(), Ordering::SeqCst);
            }
            2.. => {
                let (lock, ready) = &*self.release;
                *lock.lock().unwrap() = true;
                ready.notify_all();
            }
            _ => {}
        }
        request_id(call)
    }
}

#[test]
fn one_waiting_factory_call_does_not_serialize_another_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let timed_out = Arc::new(AtomicBool::new(false));
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components
                .observability()
                .request_id_factory(BlockingFactory {
                    calls: Arc::clone(&calls),
                    release: Arc::clone(&release),
                    timed_out: Arc::clone(&timed_out),
                });
        })
        .without_context()
        .build()
        .unwrap();
    assert!(endpoint.request_telemetry(None).request_id().is_some());

    let start = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let endpoint = endpoint.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                endpoint.request_telemetry(None)
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    for handle in handles {
        assert!(handle.join().unwrap().request_id().is_some());
    }
    assert!(!timed_out.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

fn request_id(call: usize) -> Result<RequestId, RequestIdFactoryError> {
    RequestId::new(format!("request-{call:016x}")).map_err(RequestIdFactoryError::new)
}
