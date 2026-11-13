use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::socket::{Socket, config};

mod close;
mod inbound;
mod observation;

#[derive(Clone)]
struct TestOwner {
    bytes: Bytes,
    _release: Arc<ReleaseMarker>,
}

struct ReleaseMarker(Arc<AtomicUsize>);

impl Drop for ReleaseMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl AsRef<[u8]> for TestOwner {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

struct ChunkedIo {
    inner: DuplexStream,
    max_write: usize,
}

impl AsyncRead for ChunkedIo {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ChunkedIo {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_write(context, &bytes[..bytes.len().min(this.max_write)])
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}

struct FailingWrite(DuplexStream);

impl AsyncRead for FailingWrite {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(context, buffer)
    }
}

impl AsyncWrite for FailingWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed")))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed")))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn owner(text: &'static str) -> (TestOwner, Arc<AtomicUsize>) {
    let drops = Arc::new(AtomicUsize::new(0));
    (
        TestOwner {
            bytes: Bytes::from_static(text.as_bytes()),
            _release: Arc::new(ReleaseMarker(Arc::clone(&drops))),
        },
        drops,
    )
}

async fn client(raw: DuplexStream) -> WebSocketStream<DuplexStream> {
    WebSocketStream::from_raw_socket(raw, Role::Client, None).await
}

async fn send(socket: &mut WebSocketStream<DuplexStream>, message: Message) {
    poll_fn(|context| Pin::new(&mut *socket).poll_ready(context))
        .await
        .expect("client ready");
    Pin::new(&mut *socket)
        .start_send(message)
        .expect("client send");
    poll_fn(|context| Pin::new(&mut *socket).poll_flush(context))
        .await
        .expect("client flush");
}

async fn receive(socket: &mut WebSocketStream<DuplexStream>) -> Message {
    poll_fn(|context| Pin::new(&mut *socket).poll_next(context))
        .await
        .expect("open client")
        .expect("valid message")
}

async fn write<S>(socket: &mut Socket<S, TestOwner>) -> usize
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut accepted = 0;
    loop {
        let progress = poll_fn(|context| socket.poll_write_inner(context))
            .await
            .expect("server write");
        accepted += progress.accepted;
        if progress.flushed {
            return accepted;
        }
    }
}

#[tokio::test]
async fn partial_socket_writes_count_only_text_and_hold_owner_until_flush() {
    let (server_raw, client_raw) = tokio::io::duplex(8);
    let mut server = Socket::from_stream(ChunkedIo {
        inner: server_raw,
        max_write: 2,
    })
    .await;
    let mut client = client(client_raw).await;
    let (owner, drops) = owner("partial application text");

    poll_fn(|context| server.poll_ready_inner(context))
        .await
        .expect("server ready");
    server.start_owner(owner).expect("start server text");
    assert_eq!(drops.load(Ordering::SeqCst), 0);

    let (accepted, received) = tokio::join!(write(&mut server), receive(&mut client));
    assert_eq!(accepted, b"partial application text".len());
    assert_eq!(received, Message::Text("partial application text".into()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn automatic_control_output_is_excluded_from_application_progress() {
    let (server_raw, client_raw) = tokio::io::duplex(64);
    let mut server = Socket::from_stream(server_raw).await;
    let mut client = client(client_raw).await;
    send(&mut client, Message::Ping(Bytes::from_static(b"probe"))).await;
    send(&mut client, Message::Text("inbound".into())).await;

    let inbound = poll_fn(|context| server.poll_receive_inner(context))
        .await
        .expect("server receive");
    assert!(
        matches!(inbound, Some(chigiri::__streaming::WebSocketInbound::Text(text)) if text == "inbound")
    );

    let (owner, drops) = owner("outbound");
    poll_fn(|context| server.poll_ready_inner(context))
        .await
        .expect("server ready");
    server.start_owner(owner).expect("start server text");
    let receive_both = async {
        let first = receive(&mut client).await;
        let second = receive(&mut client).await;
        (first, second)
    };
    let (accepted, (first, second)) = tokio::join!(write(&mut server), receive_both);
    assert_eq!(accepted, b"outbound".len());
    assert_eq!(first, Message::Pong(Bytes::from_static(b"probe")));
    assert_eq!(second, Message::Text("outbound".into()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn transport_failure_retains_original_owner_until_socket_drop() {
    let (server_raw, _client_raw) = tokio::io::duplex(64);
    let mut server = Socket::from_stream(FailingWrite(server_raw)).await;
    let (owner, drops) = owner("retained on failure");

    let result = server.start_owner(owner);
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(server);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn binary_and_oversized_input_are_bounded_before_retention() {
    let (server_raw, client_raw) = tokio::io::duplex(2 * 1024 * 1024);
    let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
    let mut client = client(client_raw).await;
    send(
        &mut client,
        Message::Binary(Bytes::from_static(b"not retained")),
    )
    .await;
    let inbound = poll_fn(|context| server.poll_receive_inner(context))
        .await
        .expect("binary classification");
    assert!(matches!(
        inbound,
        Some(chigiri::__streaming::WebSocketInbound::Binary)
    ));

    send(
        &mut client,
        Message::Text("x".repeat(1024 * 1024 + 1).into()),
    )
    .await;
    let Err(error) = poll_fn(|context| server.poll_receive_inner(context)).await else {
        panic!("oversized input must fail");
    };
    assert_eq!(error, chigiri::__streaming::WebSocketIoError::Protocol);
}

#[tokio::test]
async fn control_only_receive_work_yields_after_the_fixed_batch() {
    let (server_raw, client_raw) = tokio::io::duplex(64 * 1024);
    let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
    let mut client = client(client_raw).await;
    for _ in 0..33 {
        send(&mut client, Message::Ping(Bytes::new())).await;
    }

    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(server.poll_receive_inner(&mut context).is_pending());

    send(&mut client, Message::Text("after controls".into())).await;
    let inbound = poll_fn(|context| server.poll_receive_inner(context))
        .await
        .expect("text after bounded control work");
    assert!(
        matches!(inbound, Some(chigiri::__streaming::WebSocketInbound::Text(text)) if text == "after controls")
    );
}

#[test]
fn upstream_buffers_use_the_frozen_hard_frame_ceiling() {
    let config = config();
    assert_eq!(config.read_buffer_size, 16 * 1024);
    assert_eq!(config.write_buffer_size, 0);
    assert_eq!(config.max_write_buffer_size, 1024 * 1024 + 10 + 127);
    assert_eq!(config.max_message_size, Some(1024 * 1024));
    assert_eq!(config.max_frame_size, Some(1024 * 1024));
    assert!(!config.accept_unmasked_frames);
}
