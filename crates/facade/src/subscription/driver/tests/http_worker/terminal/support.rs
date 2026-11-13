use std::future::{Future, pending};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader,
    SubscriptionTransport, TransportRequest,
};

use super::super::super::deadlines::{EVENT_TIMEOUT, REQUEST_TIMEOUT, SETUP_TIMEOUT};
use super::super::super::state::AppContext;
use crate::transport::EndpointState;
use crate::transport::streaming::{PendingHttpSubscription, SubscriptionHttpIngress};

pub(super) use super::super::support::{INVALID, VALID, drain, endpoint, run, runtime};

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
static DEADLINES: LazyLock<TerminalDeadlines> = LazyLock::new(TerminalDeadlines::new);

pub(super) struct DeadlineFixture {
    _guard: MutexGuard<'static, ()>,
    pub(super) deadlines: &'static TerminalDeadlines,
}

impl DeadlineFixture {
    pub(super) fn new() -> Self {
        let guard = FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        DEADLINES.reset();
        Self {
            _guard: guard,
            deadlines: &DEADLINES,
        }
    }
}

pub(super) struct TerminalDeadlines {
    pub(super) overall: ManualDeadline,
    pub(super) setup: ManualDeadline,
    pub(super) event: ManualDeadline,
}

impl TerminalDeadlines {
    fn new() -> Self {
        Self {
            overall: ManualDeadline::new(),
            setup: ManualDeadline::new(),
            event: ManualDeadline::new(),
        }
    }

    fn reset(&self) {
        self.overall.reset();
        self.setup.reset();
        self.event.reset();
    }
}

impl DeadlineFactory for TerminalDeadlines {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if duration == REQUEST_TIMEOUT {
            self.overall.future()
        } else if duration == SETUP_TIMEOUT {
            self.setup.future()
        } else if duration == EVENT_TIMEOUT {
            self.event.future()
        } else if duration == Duration::from_secs(5) {
            Box::pin(RealDeadlineFuture::new(duration))
        } else {
            Box::pin(pending())
        }
    }
}

struct RealDeadlineFuture {
    deadline: Instant,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl RealDeadlineFuture {
    fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            sleep: None,
        }
    }
}

impl Future for RealDeadlineFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        let deadline = self.deadline;
        let sleep = self
            .sleep
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline.into())));
        sleep.as_mut().poll(context)
    }
}

pub(super) struct ManualDeadline {
    ready: AtomicBool,
    creations: AtomicUsize,
    polls: AtomicUsize,
    drops: AtomicUsize,
    waker: Mutex<Option<Waker>>,
}

impl ManualDeadline {
    const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            creations: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            drops: AtomicUsize::new(0),
            waker: Mutex::new(None),
        }
    }

    fn future(&self) -> BoxFuture<'_, ()> {
        self.creations.fetch_add(1, Ordering::SeqCst);
        Box::pin(ManualDeadlineFuture { deadline: self })
    }

    pub(super) fn trigger(&self) {
        self.ready.store(true, Ordering::SeqCst);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub(super) fn creations(&self) -> usize {
        self.creations.load(Ordering::SeqCst)
    }

    pub(super) fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }

    pub(super) fn drops(&self) -> usize {
        self.drops.load(Ordering::SeqCst)
    }

    fn reset(&self) {
        self.ready.store(false, Ordering::SeqCst);
        self.creations.store(0, Ordering::SeqCst);
        self.polls.store(0, Ordering::SeqCst);
        self.drops.store(0, Ordering::SeqCst);
        *self.waker.lock().unwrap() = None;
    }
}

struct ManualDeadlineFuture<'a> {
    deadline: &'a ManualDeadline,
}

impl Future for ManualDeadlineFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.deadline.polls.fetch_add(1, Ordering::SeqCst);
        if self.deadline.ready.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            *self.deadline.waker.lock().unwrap() = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

impl Drop for ManualDeadlineFuture<'_> {
    fn drop(&mut self) {
        self.deadline.drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct WakeLatch {
    wakes: AtomicUsize,
    wait: Mutex<()>,
    signal: Condvar,
}

impl WakeLatch {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            wakes: AtomicUsize::new(0),
            wait: Mutex::new(()),
            signal: Condvar::new(),
        })
    }

    pub(super) fn count(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }

    pub(super) fn wait_after(&self, before: usize) {
        let guard = self.wait.lock().unwrap();
        let (guard, timeout) = self
            .signal
            .wait_timeout_while(guard, Duration::from_secs(2), |_| self.count() == before)
            .unwrap();
        drop(guard);
        assert!(!timeout.timed_out(), "preparation future was not woken");
    }
}

impl Wake for WakeLatch {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        self.signal.notify_all();
    }
}

pub(super) fn poll_once<F: Future>(
    runtime: &tokio::runtime::Runtime,
    future: Pin<&mut F>,
    wake: &Arc<WakeLatch>,
) -> Poll<F::Output> {
    let _entered = runtime.enter();
    future.poll(&mut Context::from_waker(&Waker::from(Arc::clone(wake))))
}

pub(super) async fn pending_with_deadlines(
    endpoint: Arc<EndpointState<AppContext>>,
    profile: SubscriptionTransport,
    body: &[u8],
    deadlines: &'static dyn DeadlineFactory,
) -> PendingHttpSubscription<'static, AppContext> {
    let accept = match profile {
        SubscriptionTransport::ServerSentEvents => "text/event-stream",
        SubscriptionTransport::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        SubscriptionTransport::WebSocket => unreachable!(),
    };
    let metadata = ApplicationMetadata::default();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", accept.as_bytes()),
        RequestHeader::new("authorization", b"original-secret"),
    ];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    match crate::transport::streaming::handle(
        endpoint,
        transport,
        BufferedBody::new(body.to_vec()),
        deadlines,
    )
    .await
    {
        SubscriptionHttpIngress::Pending(pending) => *pending,
        SubscriptionHttpIngress::Finite(response) => panic!("unexpected finite {response:?}"),
    }
}
