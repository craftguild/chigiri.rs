use std::future::poll_fn;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use tokio::io::AsyncWrite;

use super::{Socket, client, owner, receive};
use crate::websocket::observed::ObservedIo;

struct VectoredWriter(usize);

impl AsyncWrite for VectoredWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(bytes.len().min(self.0)))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let total = buffers.iter().map(|buffer| buffer.len()).sum::<usize>();
        Poll::Ready(Ok(total.min(self.0)))
    }
}

#[test]
fn vectored_prefix_reports_only_the_accepted_text_payload() {
    let mut observed = ObservedIo::new(VectoredWriter(4));
    observed.arm_text(3).expect("arm text");
    let header = [0x81, 0x03];
    let payload = *b"abc";
    let slices = [IoSlice::new(&header), IoSlice::new(&payload)];
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    let accepted = Pin::new(&mut observed).poll_write_vectored(&mut context, &slices);
    assert!(matches!(accepted, Poll::Ready(Ok(4))));
    assert_eq!(observed.take_application_bytes(), 2);
    assert!(matches!(
        Pin::new(&mut observed).poll_write(&mut context, b"c"),
        Poll::Ready(Ok(1))
    ));
    assert_eq!(observed.take_application_bytes(), 1);
    observed.finish_text_flush().expect("complete text");
}

#[tokio::test]
async fn close_destroys_the_socket_before_the_original_owner() {
    let (server_raw, client_raw) = tokio::io::duplex(64);
    let mut server = Socket::from_stream(server_raw).await;
    let mut client = client(client_raw).await;
    let (owner, drops) = owner("held through close");
    poll_fn(|context| server.poll_ready_inner(context))
        .await
        .expect("server ready");
    server.start_owner(owner).expect("start server text");
    let close = poll_fn(|context| server.poll_close_inner(context, None));
    let peer = async {
        assert!(matches!(
            receive(&mut client).await,
            tokio_tungstenite::tungstenite::Message::Text(_)
        ));
        assert!(matches!(
            receive(&mut client).await,
            tokio_tungstenite::tungstenite::Message::Close(_)
        ));
    };
    let (closed, ()) = tokio::join!(close, peer);
    closed.expect("server close");
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(server);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
