//! Native peer closure is finalized once without releasing current text early.

use super::*;
use std::time::Duration;

async fn peer_close(socket: &mut WebSocketStream<DuplexStream>) {
    poll_fn(|cx| Pin::new(&mut *socket).poll_close(cx))
        .await
        .expect("peer close frame flush");
}

#[tokio::test]
async fn peer_close_drains_its_automatic_reply_and_finalization_is_idempotent() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (server_raw, client_raw) = tokio::io::duplex(256);
        let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
        let mut peer = client(client_raw).await;
        peer_close(&mut peer).await;
        assert!(
            poll_fn(|cx| server.poll_receive_inner(cx))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            poll_fn(|cx| server.poll_close_inner(cx, None)).await,
            Ok(())
        );
        assert!(matches!(receive(&mut peer).await, Message::Close(_)));
        assert_eq!(
            poll_fn(|cx| server.poll_close_inner(cx, None)).await,
            Ok(())
        );
    })
    .await
    .expect("native close fixture cannot wait for unrelated peer traffic");
}

#[tokio::test]
async fn peer_close_keeps_current_physical_text_until_socket_destruction() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (server_raw, client_raw) = tokio::io::duplex(256);
        let mut server = Socket::from_stream(server_raw).await;
        let mut peer = client(client_raw).await;
        let (text, drops) = owner("text retained across native close");
        poll_fn(|cx| server.poll_ready_inner(cx)).await.unwrap();
        server.start_owner(text).unwrap();
        peer_close(&mut peer).await;
        assert!(
            poll_fn(|cx| server.poll_receive_inner(cx))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            poll_fn(|cx| server.poll_close_inner(cx, None)).await,
            Ok(())
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "native finalization cannot release its current physical owner"
        );
        drop(server);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    })
    .await
    .expect("retained-owner native close fixture is bounded");
}
