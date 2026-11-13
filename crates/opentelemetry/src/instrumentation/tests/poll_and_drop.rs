use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};

use chigiri_runtime::instrumentation::{Instrumentation, InstrumentationKind};
use chigiri_runtime::{LifecycleResult, SamplingDecision};
use opentelemetry::Context;
use opentelemetry::trace::{TraceContextExt, TraceId};

use super::support::{parent, provider};

#[derive(Debug, Eq, PartialEq)]
struct Observation {
    trace: TraceId,
    suppressed: bool,
}

fn current() -> Observation {
    let context = Context::current();
    Observation {
        trace: context.span().span_context().trace_id(),
        suppressed: context.is_telemetry_suppressed(),
    }
}

struct Probe {
    polls: usize,
    panic_on_poll: bool,
    seen: Arc<Mutex<Vec<Observation>>>,
}

impl Future for Probe {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Self::Output> {
        self.seen.lock().unwrap().push(current());
        assert!(!self.panic_on_poll, "intentional application poll panic");
        self.polls += 1;
        if self.polls == 1 {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.seen.lock().unwrap().push(current());
    }
}

#[test]
fn released_context_is_owned_across_pending_thread_movement_and_destruction() {
    for suppressed in [false, true] {
        let (_owner, backend, records) = provider();
        let base = parent(4, true);
        let base = if suppressed {
            base.with_telemetry_suppressed()
        } else {
            base
        };
        let values = {
            let _guard = base.attach();
            backend.capture(&())
        };
        let scope = backend.start(
            &values,
            InstrumentationKind::Request,
            SamplingDecision::Record,
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let future = scope.instrument(Probe {
            polls: 0,
            panic_on_poll: false,
            seen: Arc::clone(&seen),
        });
        let mut future = std::thread::spawn(move || {
            let _worker = parent(5, true).with_telemetry_suppressed().attach();
            let mut future = Box::pin(future);
            assert!(
                future
                    .as_mut()
                    .poll(&mut TaskContext::from_waker(Waker::noop()))
                    .is_pending()
            );
            assert_eq!(
                current(),
                Observation {
                    trace: TraceId::from_bytes([5; 16]),
                    suppressed: true
                }
            );
            future
        })
        .join()
        .unwrap();
        let _worker = parent(6, true).attach();
        assert!(
            future
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_ready()
        );
        drop(future);
        assert_eq!(
            current(),
            Observation {
                trace: TraceId::from_bytes([6; 16]),
                suppressed: false
            }
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        for observation in seen.iter() {
            assert_eq!(
                *observation,
                Observation {
                    trace: TraceId::from_bytes([4; 16]),
                    suppressed
                }
            );
        }
        scope.finish(LifecycleResult::Succeeded);
        drop(scope);
        assert_eq!(records.take().len(), usize::from(!suppressed));
    }
}

#[test]
fn application_unwind_restores_ambient_and_last_owner_ends_once() {
    let (_owner, backend, records) = provider();
    let scope = backend.start(
        &backend.capture(&()),
        InstrumentationKind::Request,
        SamplingDecision::Record,
    );
    let expected = scope.context().parent().unwrap().trace_id();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut future = Box::pin(scope.instrument(Probe {
        polls: 0,
        panic_on_poll: true,
        seen: Arc::clone(&seen),
    }));
    drop(scope);
    let _worker = parent(7, true).with_telemetry_suppressed().attach();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = future
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()));
    }));
    assert!(failure.is_err());
    drop(future);
    assert_eq!(
        current(),
        Observation {
            trace: TraceId::from_bytes([7; 16]),
            suppressed: true
        }
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(
        seen.iter()
            .all(|observation| observation.trace.to_bytes() == expected && !observation.suppressed)
    );
    assert_eq!(records.take().len(), 1);
}
