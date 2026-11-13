use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::body::Bytes;
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::socket::{Socket, config};

mod close;
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

async fn receive(socket: &mut WebSocketStream<DuplexStream>) -> Message {
    poll_fn(|context| Pin::new(&mut *socket).poll_next(context))
        .await
        .expect("open client")
        .expect("valid message")
}

async fn write(socket: &mut Socket<DuplexStream, TestOwner>) -> usize {
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
async fn successful_flush_counts_text_and_releases_original_owner() {
    let (server_raw, client_raw) = tokio::io::duplex(64);
    let mut server = Socket::from_stream(server_raw).await;
    let mut client = client(client_raw).await;
    let (owner, drops) = owner("axum application text");

    poll_fn(|context| server.poll_ready_inner(context))
        .await
        .expect("server ready");
    server.start_owner(owner).expect("start server text");
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let (accepted, received) = tokio::join!(write(&mut server), receive(&mut client));

    assert_eq!(accepted, b"axum application text".len());
    assert_eq!(received, Message::Text("axum application text".into()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn inbound_binary_has_no_retained_payload() {
    let (server_raw, client_raw) = tokio::io::duplex(64);
    let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
    let mut client = client(client_raw).await;

    poll_fn(|context| Pin::new(&mut client).poll_ready(context))
        .await
        .expect("client ready");
    Pin::new(&mut client)
        .start_send(Message::Binary(Bytes::from_static(b"discarded")))
        .expect("client send");
    poll_fn(|context| Pin::new(&mut client).poll_flush(context))
        .await
        .expect("client flush");
    let inbound = poll_fn(|context| server.poll_receive_inner(context)).await;
    assert!(matches!(
        inbound.expect("server receive"),
        Some(chigiri::__streaming::WebSocketInbound::Binary)
    ));
}

#[tokio::test]
async fn oversized_input_maps_to_the_closed_protocol_error() {
    let (server_raw, client_raw) = tokio::io::duplex(2 * 1024 * 1024);
    let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
    let mut client = client(client_raw).await;

    poll_fn(|context| Pin::new(&mut client).poll_ready(context))
        .await
        .expect("client ready");
    Pin::new(&mut client)
        .start_send(Message::Text("x".repeat(1024 * 1024 + 1).into()))
        .expect("client send");
    poll_fn(|context| Pin::new(&mut client).poll_flush(context))
        .await
        .expect("client flush");

    let Err(error) = poll_fn(|context| server.poll_receive_inner(context)).await else {
        panic!("oversized input must fail");
    };
    assert_eq!(error, chigiri::__streaming::WebSocketIoError::Protocol);
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
