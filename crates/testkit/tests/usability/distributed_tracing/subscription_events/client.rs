//! Existing protocol frames carry ordinary requested data, with no tracing payload.

use std::net::SocketAddr;

use chigiri_http::SubscriptionTransport;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

const DOCUMENT: &str = "subscription { ticks { value padding } }";

pub(super) async fn events(
    address: SocketAddr,
    profile: SubscriptionTransport,
    case: &str,
) -> Vec<Value> {
    match profile {
        SubscriptionTransport::WebSocket => websocket(address).await,
        _ => http(address, profile, case).await,
    }
}

async fn websocket(address: SocketAddr) -> Vec<Value> {
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
    let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.status(), 101);
    socket
        .send(Message::Text("{\"type\":\"connection_init\"}".into()))
        .await
        .unwrap();
    let ack = socket.next().await.unwrap().unwrap();
    assert!(ack.to_text().unwrap().contains("connection_ack"));
    let request =
        serde_json::json!({"id":"events","type":"subscribe","payload":{"query":DOCUMENT}});
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();
    let mut events = Vec::new();
    for _ in 0..2 {
        let message = socket.next().await.unwrap().unwrap();
        let value: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        assert_eq!(value["id"], "events");
        assert_eq!(value["type"], "next");
        events.push(value["payload"].clone());
    }
    let message = socket.next().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(message.to_text().unwrap()).unwrap()["type"],
        "complete"
    );
    socket.close(None).await.unwrap();
    events
}

async fn http(address: SocketAddr, profile: SubscriptionTransport, case: &str) -> Vec<Value> {
    let accept = match profile {
        SubscriptionTransport::ServerSentEvents => "text/event-stream",
        SubscriptionTransport::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        _ => unreachable!(),
    };
    let body = serde_json::json!({"query":DOCUMENT}).to_string();
    let request = format!(
        "POST /graphql HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nx-chigiri-preflight: 1\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        assert!(bytes.len() + count <= 64 * 1024);
        bytes.extend_from_slice(&buffer[..count]);
    }
    let response = super::super::client::parse(&bytes);
    assert_eq!(response.status, 200);
    let text = std::str::from_utf8(&response.body).unwrap();
    let events = if profile == SubscriptionTransport::ServerSentEvents {
        let entries = text
            .split("\n\n")
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 3, "{case}: decoded SSE entries {entries:?}");
        assert!(entries[2].starts_with("event: complete\n"));
        entries[..2]
            .iter()
            .map(|entry| {
                let body = entry.strip_prefix("event: next\ndata: ").unwrap();
                serde_json::from_str(body).unwrap()
            })
            .collect::<Vec<Value>>()
    } else {
        assert!(text.ends_with("--graphql--"));
        text.split("--graphql\r\n")
            .skip(1)
            .map(|part| {
                let (header, body) = part.split_once("\r\n\r\n").unwrap();
                assert_eq!(header, "Content-Type: application/json");
                let body = body
                    .split_once("\r\n--graphql--")
                    .map_or(body, |(body, _)| body)
                    .trim_end_matches("\r\n");
                serde_json::from_str::<Value>(body).unwrap()["payload"].clone()
            })
            .collect()
    };
    assert_eq!(events.len(), 2);
    events
}
