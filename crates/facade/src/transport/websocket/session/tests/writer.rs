//! Successful payload flush is distinct from physical buffer retention.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::super::writer::OpeningWrite;
use super::super::{
    WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame, WebSocketWriteProgress,
};
use crate::transport::websocket::WebSocketConnectionClose;

#[derive(Default)]
struct Deadlines(Arc<AtomicBool>);

impl DeadlineFactory for Deadlines {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        let expired = Arc::clone(&self.0);
        Box::pin(std::future::poll_fn(move |_| {
            if expired.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

#[derive(Default)]
struct Socket {
    retained: Option<WebSocketWireFrame>,
    flushed: bool,
}

impl WebSocketIo for Socket {
    fn poll_receive(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        Poll::Pending
    }

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WebSocketIoError>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        frame: WebSocketWireFrame,
    ) -> Result<(), WebSocketIoError> {
        assert!(self.retained.is_none());
        self.retained = Some(frame);
        Ok(())
    }

    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        assert!(
            !self.flushed,
            "a successfully flushed frame must not be written twice"
        );
        self.flushed = true;
        Poll::Ready(Ok(WebSocketWriteProgress {
            accepted: self.retained.as_ref().unwrap().as_bytes().len(),
            flushed: true,
        }))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn flushed_retained_frame_does_not_extend_or_trigger_send_progress_timeout() {
    let (owner, outbox, progress, frame) = super::event();
    drop((outbox, progress));
    let deadlines = Box::leak(Box::new(Deadlines::default()));
    let timeout = Duration::from_secs(10);
    let mut writer = OpeningWrite::begin(frame, None, deadlines, timeout);
    let mut socket = Socket::default();
    let mut context = Context::from_waker(Waker::noop());
    assert!(
        writer
            .poll(&mut socket, &mut context, deadlines, timeout)
            .is_pending()
    );
    deadlines.0.store(true, Ordering::SeqCst);
    assert!(
        writer
            .poll(&mut socket, &mut context, deadlines, timeout)
            .is_pending()
    );
    assert!(owner.delivery().byte_owner().used() > 0);
    drop(socket.retained.take());
    assert_eq!(
        writer.poll(&mut socket, &mut context, deadlines, timeout),
        Poll::Ready(Ok(true))
    );
    assert_eq!(owner.delivery().byte_owner().used(), 0);
}
