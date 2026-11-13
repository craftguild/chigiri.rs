//! Test-only upgrade and root-session ownership fixture.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri::__streaming::{ClaimedWebSocketUpgrade, HttpStreamingError, WebSocketSessionDriver};
use chigiri::Endpoint;
use chigiri_http::BoxFuture;
use hyper::upgrade::{OnUpgrade, Upgraded};

use super::port::Receiver;
use crate::deadline::TokioDeadline;
use crate::websocket::{AxumWebSocketSocket, websocket_socket};

static DEADLINES: TokioDeadline = TokioDeadline;

pub(super) struct UpgradeEntry {
    pub(super) seed: ClaimedWebSocketUpgrade<'static>,
    pub(super) upgrade: OnUpgrade,
}

enum DriverState<C: Send + Sync + 'static> {
    Waiting(Receiver<UpgradeEntry>),
    Upgrading(UpgradeEntry),
    Wrapping {
        seed: ClaimedWebSocketUpgrade<'static>,
        socket: BoxFuture<'static, AxumWebSocketSocket>,
    },
    Session(WebSocketSessionDriver<C, AxumWebSocketSocket>),
    Done,
}

/// Test-polled owner for upgrade, session and physical cleanup boundaries.
#[must_use = "the upgrade handoff must be polled beside its HTTP connection"]
pub struct AxumWebSocketConnectionDriver<C: Send + Sync + 'static> {
    endpoint: Option<Endpoint<C>>,
    state: DriverState<C>,
    result: Option<Result<(), HttpStreamingError>>,
}

impl<C: Send + Sync + 'static> AxumWebSocketConnectionDriver<C> {
    /// Selects application shutdown and begins bounded common session cleanup.
    /// Continue polling the driver within the original shutdown deadline.
    pub fn shutdown(&mut self) {
        if let DriverState::Session(session) = &mut self.state {
            session.shutdown();
        } else if self.result.is_none() {
            let _ = self.finish(Ok(()));
        }
    }

    /// Selects a known failed application connection before physical Drop.
    pub fn transport_closed(&mut self) {
        if let DriverState::Session(session) = &mut self.state {
            session.transport_closed();
        } else if self.result.is_none() {
            let _ = self.finish(Ok(()));
        }
    }

    pub(super) const fn new(endpoint: Endpoint<C>, receiver: Receiver<UpgradeEntry>) -> Self {
        Self {
            endpoint: Some(endpoint),
            state: DriverState::Waiting(receiver),
            result: None,
        }
    }

    fn finish(
        &mut self,
        result: Result<(), HttpStreamingError>,
    ) -> Poll<Result<(), HttpStreamingError>> {
        self.endpoint.take();
        self.state = DriverState::Done;
        self.result = Some(result);
        Poll::Ready(result)
    }
}

impl<C: Send + Sync + 'static> Future for AxumWebSocketConnectionDriver<C> {
    type Output = Result<(), HttpStreamingError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        if let Some(result) = this.result {
            return Poll::Ready(result);
        }
        loop {
            match &mut this.state {
                DriverState::Waiting(receiver) => match receiver.poll_take(context) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(Some(entry))) => {
                        this.state = DriverState::Upgrading(entry);
                    }
                    Poll::Ready(Ok(None)) => return this.finish(Ok(())),
                    Poll::Ready(Err(error)) => return this.finish(Err(error)),
                },
                DriverState::Upgrading(_) => {
                    let DriverState::Upgrading(mut entry) =
                        std::mem::replace(&mut this.state, DriverState::Done)
                    else {
                        unreachable!();
                    };
                    if entry.seed.poll_upgrade_deadline(context).is_ready() {
                        return this.finish(Err(HttpStreamingError::TimedOut));
                    }
                    match Pin::new(&mut entry.upgrade).poll(context) {
                        Poll::Pending => {
                            this.state = DriverState::Upgrading(entry);
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(_)) => {
                            return this.finish(Err(HttpStreamingError::Closed));
                        }
                        Poll::Ready(Ok(upgraded)) => {
                            this.state = DriverState::Wrapping {
                                seed: entry.seed,
                                socket: socket_future(upgraded),
                            };
                        }
                    }
                }
                DriverState::Wrapping { .. } => {
                    let DriverState::Wrapping {
                        mut seed,
                        mut socket,
                    } = std::mem::replace(&mut this.state, DriverState::Done)
                    else {
                        unreachable!();
                    };
                    if seed.poll_upgrade_deadline(context).is_ready() {
                        return this.finish(Err(HttpStreamingError::TimedOut));
                    }
                    match socket.as_mut().poll(context) {
                        Poll::Pending => {
                            // The pinned no-handshake wrapper is Ready on its first
                            // poll. Keep deadline precedence if upstream changes.
                            this.state = DriverState::Wrapping { seed, socket };
                            return Poll::Pending;
                        }
                        Poll::Ready(socket) => {
                            let endpoint = this.endpoint.take().expect("live endpoint handoff");
                            match endpoint.__websocket_session(seed, socket, &DEADLINES) {
                                Ok(session) => {
                                    drop(endpoint);
                                    this.state = DriverState::Session(session);
                                }
                                Err(_) => {
                                    drop(endpoint);
                                    return this.finish(Err(HttpStreamingError::Invariant));
                                }
                            }
                        }
                    }
                }
                DriverState::Session(session) => match Pin::new(session).poll(context) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(()) => return this.finish(Ok(())),
                },
                DriverState::Done => return Poll::Ready(this.result.expect("finished driver")),
            }
        }
    }
}

fn socket_future(upgraded: Upgraded) -> BoxFuture<'static, AxumWebSocketSocket> {
    Box::pin(websocket_socket(upgraded))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    use chigiri::__streaming::{
        ClaimedWebSocketUpgrade, HttpStreamingError, WebSocketHttpVersion, WebSocketUpgradeIngress,
    };
    use chigiri::{BoxFuture, DeadlineFactory};
    use chigiri_http::{ApplicationMetadata, RequestHeader, TransportRequest};
    use hyper::Request;
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[test]
    fn pinned_no_handshake_wrapper_is_ready_on_its_first_poll() {
        let (stream, _peer) = tokio::io::duplex(64);
        let mut wrapper = Box::pin(WebSocketStream::from_raw_socket(stream, Role::Server, None));
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            wrapper.as_mut().poll(&mut context),
            Poll::Ready(_)
        ));
    }

    struct Deadlines {
        expired: Arc<AtomicBool>,
        polls: Arc<AtomicUsize>,
    }

    impl DeadlineFactory for Deadlines {
        fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
            let expired = Arc::clone(&self.expired);
            let polls = Arc::clone(&self.polls);
            Box::pin(std::future::poll_fn(move |_| {
                polls.fetch_add(1, Ordering::SeqCst);
                if expired.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }))
        }
    }

    #[tokio::test]
    async fn original_upgrade_deadline_wins_before_polling_on_upgrade() {
        let endpoint = super::super::tests::endpoint();
        let expired = Arc::new(AtomicBool::new(false));
        let polls = Arc::new(AtomicUsize::new(0));
        let deadlines = Box::leak(Box::new(Deadlines {
            expired: Arc::clone(&expired),
            polls: Arc::clone(&polls),
        }));
        let claimed = admitted(&endpoint, deadlines).await;
        let mut request = Request::new(());
        let upgrade = hyper::upgrade::on(&mut request);
        let (registration, receiver) = super::super::port::pair();
        registration
            .attach(super::UpgradeEntry {
                seed: claimed,
                upgrade,
            })
            .unwrap();
        expired.store(true, Ordering::SeqCst);
        let retained_endpoint = endpoint.clone();
        let result = super::AxumWebSocketConnectionDriver::new(endpoint, receiver).await;
        assert_eq!(result, Err(HttpStreamingError::TimedOut));
        assert!(polls.load(Ordering::SeqCst) >= 1);
        expired.store(false, Ordering::SeqCst);
        admitted(&retained_endpoint, deadlines).await.cancel();
    }

    async fn admitted(
        endpoint: &chigiri::Endpoint<()>,
        deadlines: &'static Deadlines,
    ) -> ClaimedWebSocketUpgrade<'static> {
        let headers = valid_headers();
        let metadata = ApplicationMetadata::default();
        let request = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
        let WebSocketUpgradeIngress::Admitted(seed) = endpoint
            .__admit_websocket_upgrade(request, WebSocketHttpVersion::Http11, deadlines)
            .await
        else {
            panic!("valid upgrade must be admitted");
        };
        endpoint.__claim_websocket_upgrade(seed).unwrap()
    }

    fn valid_headers() -> [RequestHeader<'static>; 7] {
        [
            RequestHeader::new("host", b"example.test"),
            RequestHeader::new("connection", b"Upgrade"),
            RequestHeader::new("upgrade", b"websocket"),
            RequestHeader::new("sec-websocket-version", b"13"),
            RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
            RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
            RequestHeader::new("x-chigiri-preflight", b"1"),
        ]
    }
}
