//! Native socket destruction can pause independently of the protocol peer.

use super::*;
use crate::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketWireFrame,
    WebSocketWriteProgress,
};

pub(super) struct BlockingSocket {
    pub(super) socket: Socket,
    pub(super) probe: Arc<State>,
    pub(super) dropped: Arc<AtomicUsize>,
    pub(super) received: Arc<AtomicUsize>,
}
impl Drop for BlockingSocket {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
        self.probe.wait_in_scalar();
    }
}
impl WebSocketIo for BlockingSocket {
    fn poll_receive(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        self.received.store(1, Ordering::SeqCst);
        Pin::new(&mut self.socket).poll_receive(cx)
    }
    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Pin::new(&mut self.socket).poll_ready(cx)
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
        Pin::new(&mut self.socket).poll_write(cx)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Pin::new(&mut self.socket).poll_close(cx, close)
    }
}
