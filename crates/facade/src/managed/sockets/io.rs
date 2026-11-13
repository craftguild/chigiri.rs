//! Native socket destruction remains guarded in its owning SDK task.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::SocketScope;
use crate::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame,
    WebSocketWriteProgress,
};

pub(super) struct GuardedSocket<S: WebSocketIo> {
    socket: Option<S>,
    scope: Arc<SocketScope>,
}
impl<S: WebSocketIo> GuardedSocket<S> {
    pub(super) fn new(socket: S, scope: Arc<SocketScope>) -> Self {
        Self {
            socket: Some(socket),
            scope,
        }
    }
    fn check<T>(&self, result: &Poll<Result<T, WebSocketIoError>>) {
        if matches!(result, Poll::Ready(Err(WebSocketIoError::Invariant))) {
            self.scope.fail();
        }
    }
}
impl<S: WebSocketIo> Drop for GuardedSocket<S> {
    fn drop(&mut self) {
        if !self
            .socket
            .take()
            .is_none_or(crate::subscription::worker::dispose_value)
        {
            self.scope.poison();
        }
    }
}
impl<S: WebSocketIo> WebSocketIo for GuardedSocket<S> {
    fn poll_receive(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        let result = Pin::new(self.socket.as_mut().expect("live native socket")).poll_receive(cx);
        self.check(&result);
        result
    }
    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        let result = Pin::new(self.socket.as_mut().expect("live native socket")).poll_ready(cx);
        self.check(&result);
        result
    }
    fn start_send(
        mut self: Pin<&mut Self>,
        frame: WebSocketWireFrame,
    ) -> Result<(), WebSocketIoError> {
        let result = Pin::new(self.socket.as_mut().expect("live native socket")).start_send(frame);
        if matches!(result, Err(WebSocketIoError::Invariant)) {
            self.scope.fail();
        }
        result
    }
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        let result = Pin::new(self.socket.as_mut().expect("live native socket")).poll_write(cx);
        self.check(&result);
        result
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        let result =
            Pin::new(self.socket.as_mut().expect("live native socket")).poll_close(cx, close);
        self.check(&result);
        result
    }
}
