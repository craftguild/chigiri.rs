use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Bytes;
use chigiri::__streaming::{
    WebSocketConnectionClose, WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame,
    WebSocketWriteProgress,
};
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Error as TungsteniteError, Message, Utf8Bytes,
        protocol::{CloseFrame, WebSocketConfig, frame::coding::CloseCode},
    },
};

use super::observed::ObservedIo;

const MAX_INBOUND_BYTES: usize = 1024 * 1024;
const READ_BUFFER_BYTES: usize = 16 * 1024;
const MAX_SERVER_HEADER_BYTES: usize = 10;
const MAX_AUTOMATIC_CONTROL_BYTES: usize = 2 + 125;
const MAX_WRITE_BUFFER_BYTES: usize =
    MAX_INBOUND_BYTES + MAX_SERVER_HEADER_BYTES + MAX_AUTOMATIC_CONTROL_BYTES;
const MAX_CONTROL_STEPS: usize = 32;

pub(super) struct Socket<S, O = WebSocketWireFrame> {
    stream: Option<WebSocketStream<ObservedIo<S>>>,
    current: Option<O>,
    closing_started: bool,
    closed: bool,
}

impl<S, O> Socket<S, O>
where
    S: AsyncRead + AsyncWrite + Unpin,
    O: AsRef<[u8]> + Clone + Send + 'static,
{
    pub(super) async fn from_stream(stream: S) -> Self {
        let stream = WebSocketStream::from_raw_socket(
            ObservedIo::new(stream),
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            Some(config()),
        )
        .await;
        Self {
            stream: Some(stream),
            current: None,
            closing_started: false,
            closed: false,
        }
    }

    pub(super) fn poll_receive_inner(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        for _ in 0..MAX_CONTROL_STEPS {
            match Pin::new(self.stream_mut()).poll_next(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    self.closed = true;
                    return Poll::Ready(Ok(None));
                }
                Poll::Ready(Some(Ok(Message::Close(_)))) => {
                    // Tungstenite already queued the native close reply.
                    self.closing_started = true;
                    return Poll::Ready(Ok(None));
                }
                Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    return Poll::Ready(Ok(Some(WebSocketInbound::Text(text.as_str().to_owned()))));
                }
                Poll::Ready(Some(Ok(Message::Binary(_)))) => {
                    return Poll::Ready(Ok(Some(WebSocketInbound::Binary)));
                }
                Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                Poll::Ready(Some(Ok(Message::Frame(_)))) => {
                    return Poll::Ready(Err(WebSocketIoError::Invariant));
                }
                Poll::Ready(Some(Err(error))) => {
                    return Poll::Ready(Err(self.map_error(error)));
                }
            }
        }
        context.waker().wake_by_ref();
        Poll::Pending
    }

    pub(super) fn poll_ready_inner(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        if self.current.is_some() {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        }
        match Pin::new(self.stream_mut()).poll_ready(context) {
            Poll::Ready(Err(error)) => Poll::Ready(Err(self.map_error(error))),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }

    pub(super) fn start_owner(&mut self, owner: O) -> Result<(), WebSocketIoError> {
        if self.current.is_some() {
            return Err(WebSocketIoError::Invariant);
        }
        let payload_bytes = owner.as_ref().len();
        let text = Utf8Bytes::try_from(Bytes::from_owner(owner.clone()))
            .map_err(|_| WebSocketIoError::Invariant)?;
        self.stream_mut()
            .get_mut()
            .arm_text(payload_bytes)
            .map_err(|_| WebSocketIoError::Invariant)?;
        self.current = Some(owner);
        Pin::new(self.stream_mut())
            .start_send(Message::Text(text))
            .map_err(|error| self.map_error(error))
    }

    pub(super) fn poll_write_inner(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        if self.current.is_none() {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        }
        let flushed = match Pin::new(self.stream_mut()).poll_flush(context) {
            Poll::Ready(Ok(())) => true,
            Poll::Ready(Err(error)) => return Poll::Ready(Err(self.map_error(error))),
            Poll::Pending => false,
        };
        let accepted = self.stream_mut().get_mut().take_application_bytes();
        if flushed {
            if self.stream_mut().get_mut().finish_text_flush().is_err() {
                return Poll::Ready(Err(WebSocketIoError::Invariant));
            }
            drop(self.current.take());
            return Poll::Ready(Ok(WebSocketWriteProgress {
                accepted,
                flushed: true,
            }));
        }
        if accepted > 0 {
            Poll::Ready(Ok(WebSocketWriteProgress {
                accepted,
                flushed: false,
            }))
        } else {
            Poll::Pending
        }
    }

    pub(super) fn poll_close_inner(
        &mut self,
        context: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.get_ref().failed())
        {
            return Poll::Ready(Err(WebSocketIoError::Invariant));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        if !self.closing_started {
            match Pin::new(self.stream_mut()).poll_ready(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(self.close_error(error)),
                Poll::Ready(Ok(())) => {}
            }
            let close = close.map(|close| CloseFrame {
                code: CloseCode::from(close.code()),
                reason: Utf8Bytes::from_static(close.reason()),
            });
            if let Err(error) = Pin::new(self.stream_mut()).start_send(Message::Close(close)) {
                return Poll::Ready(self.close_error(error));
            }
            self.closing_started = true;
        }
        match Pin::new(self.stream_mut()).poll_close(context) {
            Poll::Ready(Ok(())) => {
                self.closed = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(self.close_error(error)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn close_error(&mut self, error: TungsteniteError) -> Result<(), WebSocketIoError> {
        // Native finalization can observe an automatically completed handshake.
        // Only this operation is idempotent; other I/O keeps invariant mapping.
        if matches!(
            error,
            TungsteniteError::AlreadyClosed | TungsteniteError::ConnectionClosed
        ) && !self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.get_ref().failed())
        {
            self.closed = true;
            Ok(())
        } else {
            Err(self.map_error(error))
        }
    }

    fn stream_mut(&mut self) -> &mut WebSocketStream<ObservedIo<S>> {
        self.stream.as_mut().expect("live WebSocket stream")
    }

    fn map_error(&self, error: TungsteniteError) -> WebSocketIoError {
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.get_ref().failed())
        {
            return WebSocketIoError::Invariant;
        }
        match error {
            TungsteniteError::Capacity(_)
            | TungsteniteError::Protocol(_)
            | TungsteniteError::Utf8(_)
            | TungsteniteError::AttackAttempt => WebSocketIoError::Protocol,
            TungsteniteError::Io(_) | TungsteniteError::ConnectionClosed => {
                WebSocketIoError::Transport
            }
            _ => WebSocketIoError::Invariant,
        }
    }
}

impl<S, O> Drop for Socket<S, O> {
    fn drop(&mut self) {
        // Destroy every upstream buffer/Bytes clone before the original owner.
        drop(self.stream.take());
        drop(self.current.take());
    }
}

impl<S> WebSocketIo for Socket<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn poll_receive(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<WebSocketInbound>, WebSocketIoError>> {
        self.get_mut().poll_receive_inner(context)
    }

    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        self.get_mut().poll_ready_inner(context)
    }

    fn start_send(self: Pin<&mut Self>, frame: WebSocketWireFrame) -> Result<(), WebSocketIoError> {
        self.get_mut().start_owner(frame)
    }

    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<WebSocketWriteProgress, WebSocketIoError>> {
        self.get_mut().poll_write_inner(context)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        close: Option<WebSocketConnectionClose>,
    ) -> Poll<Result<(), WebSocketIoError>> {
        self.get_mut().poll_close_inner(context, close)
    }
}

pub(super) fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(READ_BUFFER_BYTES)
        .write_buffer_size(0)
        // This is an upstream buffer ceiling, not the configurable delivery policy.
        .max_write_buffer_size(MAX_WRITE_BUFFER_BYTES)
        .max_message_size(Some(MAX_INBOUND_BYTES))
        .max_frame_size(Some(MAX_INBOUND_BYTES))
        .accept_unmasked_frames(false)
}
