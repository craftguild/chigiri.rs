//! Native callbacks accept a stop synchronously before returning their result.

use super::*;
use crate::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketWireFrame,
    WebSocketWriteProgress,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Boundary {
    Receive,
    Ready,
    Write,
}

pub(super) struct SignalSocket {
    socket: Socket,
    control: Arc<Control>,
    boundary: Boundary,
    options: ShutdownOptions,
    signalled: Arc<AtomicUsize>,
}
impl SignalSocket {
    pub(super) fn new(
        socket: Socket,
        control: Arc<Control>,
        boundary: Boundary,
        options: ShutdownOptions,
        signalled: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            socket,
            control,
            boundary,
            options,
            signalled,
        }
    }
    fn signal(&self, boundary: Boundary) {
        if self.boundary == boundary && self.signalled.load(Ordering::SeqCst) == 0 {
            self.control
                .begin(self.options, tokio::time::Instant::now());
            self.signalled.store(1, Ordering::SeqCst);
        }
    }
}
impl WebSocketIo for SignalSocket {
    fn poll_receive(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        let result = Pin::new(&mut self.socket).poll_receive(cx);
        if matches!(result, Poll::Ready(Ok(Some(_)))) {
            self.signal(Boundary::Receive);
        }
        result
    }
    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        let result = Pin::new(&mut self.socket).poll_ready(cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.signal(Boundary::Ready);
        }
        result
    }
    fn start_send(
        mut self: Pin<&mut Self>,
        frame: WebSocketWireFrame,
    ) -> Result<(), WebSocketIoError> {
        Pin::new(&mut self.socket).start_send(frame)
    }
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        let result = Pin::new(&mut self.socket).poll_write(cx);
        if matches!(result, Poll::Ready(Ok(_))) {
            self.signal(Boundary::Write);
        }
        result
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Pin::new(&mut self.socket).poll_close(cx, close)
    }
}
