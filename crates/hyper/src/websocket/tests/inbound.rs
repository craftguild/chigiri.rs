use std::future::poll_fn;
use std::pin::Pin;

use tokio::io::AsyncWrite;

use super::{Socket, TestOwner};

async fn write_all(raw: &mut tokio::io::DuplexStream, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let accepted = poll_fn(|context| Pin::new(&mut *raw).poll_write(context, bytes))
            .await
            .expect("raw client write");
        assert!(accepted > 0);
        bytes = &bytes[accepted..];
    }
}

async fn protocol_error(bytes: &[u8]) -> chigiri::__streaming::WebSocketIoError {
    let (server_raw, mut client_raw) = tokio::io::duplex(64);
    let mut server = Socket::<_, TestOwner>::from_stream(server_raw).await;
    write_all(&mut client_raw, bytes).await;
    let Err(error) = poll_fn(|context| server.poll_receive_inner(context)).await else {
        panic!("invalid frame must fail");
    };
    error
}

#[tokio::test]
async fn invalid_utf8_and_unmasked_client_frames_are_protocol_errors() {
    let invalid_utf8 = [0x81, 0x82, 0, 0, 0, 0, 0xff, 0xff];
    assert_eq!(
        protocol_error(&invalid_utf8).await,
        chigiri::__streaming::WebSocketIoError::Protocol
    );

    let unmasked = [0x81, 0x01, b'a'];
    assert_eq!(
        protocol_error(&unmasked).await,
        chigiri::__streaming::WebSocketIoError::Protocol
    );
}
