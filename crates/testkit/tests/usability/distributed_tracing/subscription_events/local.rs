//! The existing local driver preserves context through cancellation, timeout and unwind.

#[path = "local_verify.rs"]
mod verify;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::Duration;

use chigiri::__streaming::SubscriptionInitialization;
use chigiri_http::{
    ApplicationMetadata, DeadlineFactory, RequestMethod, ResponseCompletionOutcome,
    ResponseMediaType, ResponseParts, SubscriptionTransport,
};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::TraceContextExt;

use super::super::super::support::{NeverDeadline, block_on};
use super::super::{FixedSampling, PrivateValue, parent, recording};
use super::{behavior::Mode, fixture};

const CONTEXT: Duration = Duration::from_millis(11);
const EXECUTION: Duration = Duration::from_millis(12);

#[derive(Clone, Copy, Debug)]
pub(super) enum End {
    Success,
    Reject,
    Internal,
    CancelFactory,
    TimeoutFactory,
    PanicFactory,
    CancelResolver,
    TimeoutResolver,
    PanicResolver,
}
impl End {
    fn mode(self) -> Mode {
        match self {
            Self::Success => Mode::Success,
            Self::Reject => Mode::RejectFactory,
            Self::Internal => Mode::InternalFactory,
            Self::CancelFactory | Self::TimeoutFactory => Mode::PendingFactory,
            Self::PanicFactory => Mode::PanicFactory,
            Self::CancelResolver | Self::TimeoutResolver => Mode::PendingResolver,
            Self::PanicResolver => Mode::PanicResolver,
        }
    }
}

struct Deadline(Option<Duration>);
impl DeadlineFactory for Deadline {
    fn sleep(&self, duration: Duration) -> chigiri::BoxFuture<'_, ()> {
        if Some(duration) != self.0 {
            return Box::pin(std::future::pending());
        }
        let mut first = true;
        Box::pin(std::future::poll_fn(move |context| {
            if first {
                first = false;
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }))
    }
}

struct Errors {
    calls: Arc<AtomicUsize>,
    suppressed: bool,
}
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert_eq!(
            Context::current().is_telemetry_suppressed(),
            self.suppressed
        );
        assert_eq!(event.code(), "INTERNAL_SERVER_ERROR");
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn local_events_restore_context_on_success_failure_timeout_cancel_and_unwind() {
    for profile in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        for suppressed in [false, true] {
            for has_parent in [false, true] {
                for decision in [
                    chigiri::SamplingDecision::Record,
                    chigiri::SamplingDecision::Drop,
                ] {
                    for end in [
                        End::Success,
                        End::Reject,
                        End::Internal,
                        End::CancelFactory,
                        End::TimeoutFactory,
                        End::PanicFactory,
                        End::CancelResolver,
                        End::TimeoutResolver,
                        End::PanicResolver,
                    ] {
                        run_case(profile, suppressed, has_parent, decision, end);
                    }
                }
            }
        }
    }
}

fn run_case(
    profile: SubscriptionTransport,
    suppressed: bool,
    has_parent: bool,
    decision: chigiri::SamplingDecision,
    end: End,
) {
    let (_provider, tracer, records) = recording::provider();
    let calls = Arc::clone(&tracer.calls);
    let drops = Arc::clone(&tracer.dropped);
    let errors = Arc::new(AtomicUsize::new(0));
    let mut probe = fixture::Probe::new(profile);
    probe.mode = end.mode();
    let probe = Arc::new(probe);
    let endpoint = chigiri::Endpoint::builder(fixture::resolvers(Arc::clone(&probe)))
        .inject(|components| {
            components
                .http()
                .context_timeout(CONTEXT)
                .execution_timeout(EXECUTION);
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .sampler(FixedSampling(decision))
                .error_observer(Errors {
                    calls: Arc::clone(&errors),
                    suppressed,
                });
        })
        .context_factory(fixture::Factory(Arc::clone(&probe)))
        .build()
        .unwrap();
    let metadata = ApplicationMetadata::default();
    let transport = chigiri::TransportRequest::new("POST", "/graphql", None, &[], None, &metadata);
    let start = {
        let incoming = if has_parent {
            Context::new().with_remote_span_context(parent(7))
        } else {
            Context::new()
        };
        let incoming = if suppressed {
            incoming.with_telemetry_suppressed()
        } else {
            incoming
        };
        let _guard = incoming.with_value(PrivateValue).attach();
        endpoint.__start_subscription(
            transport,
            chigiri::GraphQlRequest::new("subscription { ticks { value padding } }"),
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
            profile,
            None,
            &NeverDeadline,
        )
    };
    let SubscriptionInitialization::Ready {
        mut subscription,
        completion,
        ..
    } = block_on(start)
    else {
        panic!("setup must succeed before exercising {end:?}");
    };
    completion.finish(
        ResponseParts {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        },
        ResponseCompletionOutcome::Encoded,
    );
    let deadline = Deadline(match end {
        End::TimeoutFactory => Some(CONTEXT),
        End::TimeoutResolver => Some(EXECUTION),
        _ => None,
    });
    let mut next = subscription.next(&deadline);
    {
        let _guard = worker(8, !suppressed).attach();
        assert!(
            next.as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_worker(8, !suppressed);
    }
    std::thread::scope(|threads| {
        threads
            .spawn(|| {
                let _guard = worker(9, !suppressed).attach();
                match end {
                    End::CancelFactory | End::CancelResolver => {
                        let stage = if matches!(end, End::CancelFactory) {
                            "factory-poll"
                        } else {
                            "resolve"
                        };
                        for _ in 0..8 {
                            assert!(
                                next.as_mut()
                                    .poll(&mut TaskContext::from_waker(Waker::noop()))
                                    .is_pending()
                            );
                            if probe
                                .seen
                                .lock()
                                .unwrap()
                                .iter()
                                .any(|seen| seen.phase == probe.event(1) && seen.stage == stage)
                            {
                                break;
                            }
                        }
                        assert!(
                            probe
                                .seen
                                .lock()
                                .unwrap()
                                .iter()
                                .any(|seen| seen.phase == probe.event(1) && seen.stage == stage)
                        );
                        drop(next);
                    }
                    End::PanicFactory | End::PanicResolver => {
                        assert!(catch_unwind(AssertUnwindSafe(|| block_on(next))).is_err());
                    }
                    _ => verify::output(block_on(next), end),
                }
                assert_worker(9, !suppressed);
            })
            .join()
            .unwrap();
    });
    std::thread::scope(|threads| {
        threads
            .spawn(|| {
                let _guard = worker(9, !suppressed).attach();
                drop(subscription);
                assert_worker(9, !suppressed);
            })
            .join()
            .unwrap();
    });
    assert_eq!(
        errors.load(Ordering::SeqCst),
        usize::from(matches!(end, End::Internal))
    );
    let spans = records.take();
    assert_eq!(spans.len(), calls.load(Ordering::SeqCst));
    assert_eq!(drops.load(Ordering::SeqCst), calls.load(Ordering::SeqCst));
    verify::contexts(&spans, &probe, suppressed, has_parent, decision, end);
}

fn worker(byte: u8, suppressed: bool) -> Context {
    let context = Context::new()
        .with_remote_span_context(parent(byte))
        .with_value(PrivateValue);
    if suppressed {
        context.with_telemetry_suppressed()
    } else {
        context
    }
}
fn assert_worker(byte: u8, suppressed: bool) {
    let current = Context::current();
    assert_eq!(*current.span().span_context(), parent(byte));
    assert_eq!(current.is_telemetry_suppressed(), suppressed);
    assert!(current.get::<PrivateValue>().is_some());
}
