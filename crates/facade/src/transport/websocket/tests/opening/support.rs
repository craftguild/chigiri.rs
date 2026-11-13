use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, TransportRequest, WebSocketHttpVersion,
};

use super::super::support::{ready, valid_headers};
use crate::Endpoint;
use crate::transport::websocket::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketIoError,
    WebSocketOpeningDriver, WebSocketOpeningOutcome, WebSocketUpgradeIngress, WebSocketWireFrame,
    WebSocketWriteProgress,
};

#[derive(Default)]
pub(super) struct State {
    pub(super) incoming: VecDeque<Result<Option<WebSocketInbound>, WebSocketIoError>>,
    pub(super) current: Option<WebSocketWireFrame>,
    pub(super) retained: Option<WebSocketWireFrame>,
    pub(super) retain: bool,
    pub(super) writes: VecDeque<WebSocketWriteProgress>,
    pub(super) sent: Vec<Vec<u8>>,
    pub(super) closes: Vec<Option<u16>>,
    pub(super) close_pending: bool,
    pub(super) drop_retained_on_read: bool,
    pub(super) ready_pending: bool,
    pub(super) ready_polls: usize,
}

#[derive(Clone, Default)]
pub(super) struct Socket(pub(super) Arc<Mutex<State>>);

impl WebSocketIo for Socket {
    fn poll_receive(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        let (message, retained) = {
            let mut state = self.0.lock().unwrap();
            let message = state.incoming.pop_front();
            let retained = if state.drop_retained_on_read {
                state.drop_retained_on_read = false;
                state.retained.take()
            } else {
                None
            };
            (message, retained)
        };
        drop(retained);
        message.map_or(Poll::Pending, Poll::Ready)
    }

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WebSocketIoError>> {
        let mut state = self.0.lock().unwrap();
        state.ready_polls += 1;
        assert!(state.current.is_none());
        if state.ready_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, frame: WebSocketWireFrame) -> Result<(), WebSocketIoError> {
        let mut state = self.0.lock().unwrap();
        assert!(state.current.is_none());
        state.sent.push(frame.as_bytes().to_vec());
        if state.retain {
            assert!(state.retained.is_none());
            state.retained = Some(frame.clone());
        }
        state.current = Some(frame);
        Ok(())
    }

    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        let (progress, frame) = {
            let mut state = self.0.lock().unwrap();
            let Some(progress) = state.writes.pop_front() else {
                return Poll::Pending;
            };
            let frame = progress.flushed.then(|| state.current.take()).flatten();
            (progress, frame)
        };
        drop(frame);
        Poll::Ready(Ok(progress))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        let frame = {
            let mut state = self.0.lock().unwrap();
            state.closes.push(close.map(WebSocketConnectionClose::code));
            if state.close_pending {
                return Poll::Pending;
            }
            state.current.take()
        };
        drop(frame);
        Poll::Ready(Ok(()))
    }
}

pub(super) fn start(
    endpoint: &Endpoint<()>,
    deadlines: &'static dyn DeadlineFactory,
    socket: Socket,
) -> WebSocketOpeningDriver<Socket> {
    let headers = valid_headers(true);
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    let seed = match ready(endpoint.__admit_websocket_upgrade(
        request,
        WebSocketHttpVersion::Http11,
        deadlines,
    )) {
        WebSocketUpgradeIngress::Admitted(seed) => seed,
        WebSocketUpgradeIngress::Finite(_) => panic!("expected a charged upgrade"),
    };
    endpoint
        .__websocket_opening(
            endpoint.__claim_websocket_upgrade(seed).unwrap(),
            socket,
            deadlines,
        )
        .unwrap()
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap()
}

pub(super) fn poll_once(
    driver: &mut WebSocketOpeningDriver<Socket>,
) -> Poll<WebSocketOpeningOutcome<Socket>> {
    Pin::new(driver).poll(&mut Context::from_waker(Waker::noop()))
}

pub(super) async fn until_frame(driver: &mut WebSocketOpeningDriver<Socket>, socket: &Socket) {
    until(driver, socket, |state| state.current.is_some()).await;
}

pub(super) async fn until_ready_wait(driver: &mut WebSocketOpeningDriver<Socket>, socket: &Socket) {
    until(driver, socket, |state| state.ready_polls > 0).await;
}

async fn until(
    driver: &mut WebSocketOpeningDriver<Socket>,
    socket: &Socket,
    done: impl Fn(&State) -> bool,
) {
    tokio::time::timeout(
        Duration::from_secs(2),
        poll_fn(|context| {
            assert!(Pin::new(&mut *driver).poll(context).is_pending());
            if done(&socket.0.lock().unwrap()) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .expect("the concrete authorization worker must reach its first frame");
}

pub(super) fn queue_init(socket: &Socket) {
    socket
        .0
        .lock()
        .unwrap()
        .incoming
        .push_back(Ok(Some(WebSocketInbound::Text(
            r#"{"type":"connection_init"}"#.to_owned(),
        ))));
}

pub(super) fn flush(socket: &Socket, accepted: usize) {
    socket
        .0
        .lock()
        .unwrap()
        .writes
        .push_back(WebSocketWriteProgress {
            accepted,
            flushed: true,
        });
}

#[derive(Default)]
pub(super) struct Clock(Arc<AtomicU64>);

impl Clock {
    pub(super) fn advance_to(&self, seconds: u64) {
        let previous = self.0.swap(seconds, Ordering::SeqCst);
        assert!(seconds >= previous);
    }
}

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        let at = self.0.load(Ordering::SeqCst) + duration.as_secs();
        let now = Arc::clone(&self.0);
        Box::pin(poll_fn(move |_| {
            if now.load(Ordering::SeqCst) >= at {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}
