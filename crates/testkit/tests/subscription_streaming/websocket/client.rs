//! A bounded ecosystem client, with its TCP socket and runtime owned by the test.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, protocol::WebSocketConfig,
};

pub(super) struct Client {
    socket: WebSocketStream<tokio::net::TcpStream>,
    runtime: tokio::runtime::Runtime,
}

impl Client {
    pub(super) fn connect(address: SocketAddr) -> Self {
        Self::opening(address, Some("graphql-transport-ws"), true)
            .unwrap_or_else(|status| panic!("unexpected finite opening rejection: {status}"))
    }

    pub(super) fn assert_rejected(
        address: SocketAddr,
        protocol: Option<&str>,
        preflight: bool,
        expected: u16,
    ) {
        match Self::opening(address, protocol, preflight) {
            Err(status) => assert_eq!(status, expected),
            Ok(_) => panic!("invalid opening profile must not upgrade"),
        }
    }

    fn opening(address: SocketAddr, protocol: Option<&str>, preflight: bool) -> Result<Self, u16> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let opening = runtime.block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), async move {
                let stream = tokio::net::TcpStream::connect(address).await.unwrap();
                let mut request = format!("ws://{address}/graphql")
                    .into_client_request()
                    .unwrap();
                if preflight {
                    request
                        .headers_mut()
                        .insert("x-chigiri-preflight", "1".parse().unwrap());
                }
                if let Some(protocol) = protocol {
                    request
                        .headers_mut()
                        .insert("sec-websocket-protocol", protocol.parse().unwrap());
                }
                let config = WebSocketConfig::default()
                    .read_buffer_size(16 * 1024)
                    .write_buffer_size(0)
                    .max_message_size(Some(1024 * 1024))
                    .max_frame_size(Some(1024 * 1024))
                    .max_write_buffer_size(2 * 1024 * 1024);
                tokio_tungstenite::client_async_with_config(request, stream, Some(config)).await
            })
            .await
            .expect("bounded live WebSocket client handshake")
        });
        let (socket, response) = match opening {
            Ok(response) => response,
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                return Err(response.status().as_u16());
            }
            Err(error) => panic!("live client handshake: {error}"),
        };
        assert_eq!(response.status(), 101);
        assert_eq!(
            response.headers()["sec-websocket-protocol"],
            "graphql-transport-ws"
        );
        assert!(!response.headers().contains_key("content-length"));
        Ok(Self { socket, runtime })
    }

    pub(super) fn send(&mut self, value: Value) {
        self.frame(Message::Text(value.to_string().into()));
    }

    pub(super) fn frame(&mut self, message: Message) {
        let socket = &mut self.socket;
        self.runtime.block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), socket.send(message))
                .await
                .expect("bounded client write")
                .expect("send client JSON message");
        });
    }

    pub(super) fn initialize(&mut self) {
        self.send(serde_json::json!({"type":"connection_init"}));
        assert_eq!(self.next(), serde_json::json!({"type":"connection_ack"}));
    }

    pub(super) fn subscribe(&mut self, id: &str, query: &str) {
        self.send(serde_json::json!({"id":id,"type":"subscribe","payload":{"query":query}}));
    }

    pub(super) fn next(&mut self) -> Value {
        match self.receive() {
            Message::Text(text) => {
                serde_json::from_str(text.as_str()).expect("server JSON message")
            }
            message => panic!("expected server JSON message: {message:?}"),
        }
    }

    pub(super) fn expect_close(&mut self, expected: u16) {
        match self.receive() {
            Message::Close(Some(frame)) => {
                assert_eq!(u16::from(frame.code), expected);
                assert!(
                    !frame.reason.contains("private"),
                    "opaque failures stay internal"
                );
            }
            message => panic!("expected safe protocol close {expected}: {message:?}"),
        }
    }

    fn receive(&mut self) -> Message {
        let socket = &mut self.socket;
        self.runtime.block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), async move {
                for _ in 0..32 {
                    match socket
                        .next()
                        .await
                        .expect("server must send a protocol message")
                        .expect("read WebSocket frame")
                    {
                        Message::Ping(_) | Message::Pong(_) => {}
                        message => return message,
                    }
                }
                panic!("bounded RFC control work before the next protocol message");
            })
            .await
            .expect("bounded server protocol response")
        })
    }

    pub(super) fn close(&mut self) {
        let socket = &mut self.socket;
        self.runtime.block_on(async move {
            tokio::time::timeout(Duration::from_secs(5), socket.close(None))
                .await
                .expect("bounded client close")
                .expect("send client close");
        });
    }
}
