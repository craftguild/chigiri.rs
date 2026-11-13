//! Owned idle transports keep setup and source lifetime observable separately.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{
    Error as WebSocketError, Message, client::IntoClientRequest, error::ProtocolError,
};

#[derive(Clone, Copy, Debug)]
pub(in crate::distributed_tracing) enum Profile {
    WebSocket,
    Sse,
    Multipart,
}

pub(in crate::distributed_tracing) enum Idle {
    WebSocket(
        Box<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>,
    ),
    Http(TcpStream),
}
impl Idle {
    pub(in crate::distributed_tracing) async fn open(
        address: SocketAddr,
        profile: Profile,
    ) -> Self {
        if matches!(profile, Profile::WebSocket) {
            let mut request = format!("ws://{address}/graphql")
                .into_client_request()
                .unwrap();
            request.headers_mut().insert(
                "sec-websocket-protocol",
                "graphql-transport-ws".parse().unwrap(),
            );
            request
                .headers_mut()
                .insert("x-chigiri-preflight", "1".parse().unwrap());
            let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
            socket
                .send(Message::Text("{\"type\":\"connection_init\"}".into()))
                .await
                .unwrap();
            let message = socket.next().await.unwrap().unwrap();
            assert!(message.to_text().unwrap().contains("connection_ack"));
            return Self::WebSocket(Box::new(socket));
        }
        Self::Http(TcpStream::connect(address).await.unwrap())
    }

    pub(in crate::distributed_tracing) async fn subscribe(
        &mut self,
        address: SocketAddr,
        profile: Profile,
    ) {
        match self {
            Self::WebSocket(socket) => socket.send(Message::Text("{\"id\":\"idle\",\"type\":\"subscribe\",\"payload\":{\"query\":\"subscription { ticks { value } }\"}}".into())).await.unwrap(),
            Self::Http(stream) => {
                let accept = match profile {
                    Profile::Sse => "text/event-stream",
                    Profile::Multipart => "multipart/mixed; subscriptionSpec=1.0",
                    Profile::WebSocket => unreachable!(),
                };
                let body = "{\"query\":\"subscription { ticks { value } }\"}";
                let request = format!("POST /graphql HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nx-chigiri-preflight: 1\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    assert!(header.len() < 16 * 1024);
                    header.push(stream.read_u8().await.unwrap());
                }
                assert!(header.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&header));
            }
        }
    }

    pub(in crate::distributed_tracing) async fn close(self) {
        if let Self::WebSocket(mut socket) = self {
            socket.as_mut().close(None).await.unwrap();
        }
    }

    pub(in crate::distributed_tracing) async fn failed(self) {
        match self {
            Self::WebSocket(mut socket) => {
                for _ in 0..4 {
                    let message = match socket.next().await.unwrap() {
                        Ok(message) => message,
                        Err(WebSocketError::Protocol(
                            ProtocolError::ResetWithoutClosingHandshake,
                        )) => return,
                        Err(error) => panic!("unexpected native event failure: {error}"),
                    };
                    if let Message::Close(frame) = message {
                        assert_eq!(u16::from(frame.unwrap().code), 1011);
                        socket.flush().await.unwrap();
                        return;
                    }
                    assert!(!message.to_text().unwrap().contains("private event failure"));
                }
                panic!("failed event must close its native socket");
            }
            Self::Http(mut stream) => {
                let mut body = Vec::new();
                let mut bytes = [0; 4096];
                loop {
                    let count = stream.read(&mut bytes).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    assert!(body.len() + count <= 64 * 1024);
                    body.extend_from_slice(&bytes[..count]);
                }
                assert!(!String::from_utf8_lossy(&body).contains("private event failure"));
            }
        }
    }
}
