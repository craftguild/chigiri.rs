//! Bounded application-polled WebSocket I/O for Axum-owned upgrades.

mod observed;
mod socket;

#[cfg(test)]
mod tests;

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame,
    WebSocketWriteProgress,
};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;

use socket::Socket;

/// A bounded WebSocket socket port over one application-owned Axum upgrade.
#[doc(hidden)]
pub struct AxumWebSocketSocket {
    socket: Socket<TokioIo<Upgraded>>,
}

/// Wraps an already-upgraded application-owned Axum stream without spawning.
#[doc(hidden)]
pub async fn websocket_socket(upgraded: Upgraded) -> AxumWebSocketSocket {
    AxumWebSocketSocket {
        socket: Socket::from_stream(TokioIo::new(upgraded)).await,
    }
}

impl WebSocketIo for AxumWebSocketSocket {
    fn poll_receive(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        Pin::new(&mut self.get_mut().socket).poll_receive(context)
    }

    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Pin::new(&mut self.get_mut().socket).poll_ready(context)
    }

    fn start_send(self: Pin<&mut Self>, frame: WebSocketWireFrame) -> Result<(), WebSocketIoError> {
        Pin::new(&mut self.get_mut().socket).start_send(frame)
    }

    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        Pin::new(&mut self.get_mut().socket).poll_write(context)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        Pin::new(&mut self.get_mut().socket).poll_close(context, close)
    }
}
