//! Concrete public endpoint, registered runtime, same socket and manual timers.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, RequestHeader, TransportRequest,
    WebSocketHttpVersion,
};

use super::super::harness::session_endpoint;
use super::super::state::{AppContext, State, count};
use super::socket::{Handle, Socket};
use crate::__streaming::{
    SubscriptionRuntimeGuard, WebSocketSessionDriver, WebSocketUpgradeIngress,
};
use crate::{Endpoint, SubscriptionLimits};

pub(super) type Driver = WebSocketSessionDriver<AppContext, Socket>;

struct SourceObserver<'a>(&'a State);

impl Drop for SourceObserver<'_> {
    fn drop(&mut self) {
        self.0.observe_source(None);
    }
}

#[derive(Default)]
pub(super) struct Clock(Arc<AtomicU64>, Mutex<Vec<(u64, Duration)>>);

impl Clock {
    pub(super) fn starts_for(&self, duration: Duration) -> Vec<u64> {
        self.1
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(now, delay)| (*delay == duration).then_some(*now))
            .collect()
    }
}

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        let start = self.0.load(Ordering::SeqCst);
        self.1.lock().unwrap().push((start, duration));
        let end = start
            .checked_add(u64::try_from(duration.as_nanos()).unwrap())
            .unwrap();
        let now = Arc::clone(&self.0);
        Box::pin(poll_fn(move |_| {
            if now.load(Ordering::SeqCst) >= end {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

pub(super) struct Fixture {
    pub(super) runtime: tokio::runtime::Runtime,
    pub(super) endpoint: Endpoint<AppContext>,
    pub(super) socket: Handle,
    pub(super) driver: Option<Driver>,
    pub(super) clock: &'static Clock,
    guard: Option<SubscriptionRuntimeGuard>,
}

impl Fixture {
    pub(super) fn new(probe: Arc<State>) -> Self {
        Self::with_limits(probe, SubscriptionLimits::default())
    }

    pub(super) fn with_limits(probe: Arc<State>, limits: SubscriptionLimits) -> Self {
        Self::with_endpoint(session_endpoint(probe, limits))
    }

    pub(super) fn with_endpoint(endpoint: Endpoint<AppContext>) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .unwrap();
        let guard = endpoint.__register_subscription_runtime().unwrap();
        let clock = Box::leak(Box::<Clock>::default());
        let socket = Handle::default();
        socket.text(r#"{"type":"connection_init"}"#);
        let headers = [
            RequestHeader::new("host", b"example.test"),
            RequestHeader::new("connection", b"Upgrade"),
            RequestHeader::new("upgrade", b"websocket"),
            RequestHeader::new("sec-websocket-version", b"13"),
            RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
            RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
            RequestHeader::new("x-chigiri-preflight", b"1"),
            RequestHeader::new("authorization", b"original-secret"),
        ];
        let metadata = ApplicationMetadata::default();
        let request = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
        let seed = match runtime.block_on(endpoint.__admit_websocket_upgrade(
            request,
            WebSocketHttpVersion::Http11,
            clock,
        )) {
            WebSocketUpgradeIngress::Admitted(seed) => {
                endpoint.__claim_websocket_upgrade(seed).unwrap()
            }
            WebSocketUpgradeIngress::Finite(response) => {
                panic!("opening rejected with {}", response.status)
            }
        };
        let driver = endpoint
            .__websocket_session(seed, socket.socket(), clock)
            .unwrap();
        let mut fixture = Self {
            runtime,
            endpoint,
            socket,
            driver: Some(driver),
            clock,
            guard: Some(guard),
        };
        let handle = fixture.socket.clone();
        fixture.until(|_| !handle.messages().is_empty());
        fixture.driver.as_mut().unwrap().set_time_for_test(0);
        fixture
    }

    #[track_caller]
    pub(super) fn until(&mut self, done: impl Fn(&Driver) -> bool) {
        let caller = std::panic::Location::caller();
        let driver = self.driver.as_mut().unwrap();
        let result = self.runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(3),
                poll_fn(|context| {
                    assert!(
                        Pin::new(&mut *driver).poll(context).is_pending(),
                        "connection closed before the fixture condition"
                    );
                    if done(driver) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                }),
            )
            .await
        });
        if result.is_err() {
            let detail = {
                let state = self.socket.0.lock().unwrap();
                format!(
                    "sent={:?}, queued_input={}, current={}, closes={:?}",
                    state.sent,
                    state.input.len(),
                    state.current.is_some(),
                    state.closes
                )
            };
            panic!("bounded same-socket condition at {caller}: {detail}");
        }
    }

    pub(super) fn poll_once(&mut self) -> Poll<()> {
        let _entered = self.runtime.enter();
        Pin::new(self.driver.as_mut().unwrap()).poll(&mut Context::from_waker(Waker::noop()))
    }

    pub(super) fn until_source_pending(&mut self, probe: &State) {
        self.until_source_pending_within(probe, Duration::from_secs(3));
    }

    pub(super) fn until_source_pending_within(&mut self, probe: &State, budget: Duration) {
        let _observer = SourceObserver(probe);
        let driver = self.driver.as_mut().unwrap();
        self.runtime.block_on(async {
            tokio::time::timeout(
                budget,
                poll_fn(|context| {
                    // Worker progress need not produce transport work or wake the driver.
                    probe.observe_source(Some(context.waker()));
                    assert!(Pin::new(&mut *driver).poll(context).is_pending());
                    if probe.source_pending_observed.load(Ordering::SeqCst) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                }),
            )
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "source did not reach Pending; polls: {}",
                    count(&probe.counts.source_polls)
                )
            });
        });
    }

    pub(super) fn advance(&mut self, time: Duration) {
        let now = u64::try_from(time.as_nanos()).unwrap();
        assert!(now >= self.clock.0.load(Ordering::SeqCst));
        self.clock.0.store(now, Ordering::SeqCst);
        self.driver.as_mut().unwrap().set_time_for_test(now);
    }

    pub(super) fn until_closed(&mut self) {
        let driver = self.driver.as_mut().unwrap();
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), driver)
                .await
                .expect("connection close and cooperative cleanup must remain bounded");
        });
    }

    pub(super) fn finish(mut self) {
        self.socket.0.lock().unwrap().eof = true;
        if let Some(driver) = self.driver.take() {
            self.runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(3), driver)
                    .await
                    .unwrap();
            });
        }
        let retained = self.socket.0.lock().unwrap().retained.take();
        drop(retained);
        self.runtime
            .block_on(self.guard.take().unwrap().shutdown().unwrap())
            .unwrap();
        assert_eq!(self.endpoint.in_flight_requests(), 0);
    }
}

pub(super) fn messages_for(handle: &Handle, id: &str) -> Vec<String> {
    handle
        .messages()
        .into_iter()
        .filter(|message| message.contains(&format!(r#""id":"{id}""#)))
        .collect()
}

pub(super) fn kind(message: &str) -> &str {
    // Inspect only canonical generated protocol frames. The enclosing tests
    // assert exact data/control fragments from version-controlled fixtures.
    message
        .split(r#""type":""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}

pub(super) fn delivery_value(message: &str) -> &str {
    message
        .split(r#""extensions":{"chigiri":{"delivery":{"droppedTotal":"#)
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap()
        .trim()
}

pub(super) const SUBSCRIPTION: &str = "subscription { chosen: events(limit: 2) { value } }";
