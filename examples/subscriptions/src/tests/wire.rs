//! Bounded HTTP/1 and POST-SSE reads through the application's actual listener.

use std::net::SocketAddr;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub(super) struct Http {
    reader: BufReader<TcpStream>,
    pub(super) status: u16,
    pub(super) head: String,
    chunked: bool,
}

impl Http {
    pub(super) async fn open(
        address: SocketAddr,
        path: &str,
        document: Option<&str>,
        origin: Option<&str>,
        streaming: bool,
    ) -> Self {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let body = document
            .map(|query| serde_json::json!({"query":query}).to_string())
            .unwrap_or_default();
        let method = if document.is_some() { "POST" } else { "GET" };
        let accept = if streaming {
            "text/event-stream"
        } else {
            "application/graphql-response+json"
        };
        let origin = origin
            .map(|value| format!("Origin: {value}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\n{origin}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).await.unwrap(), 0);
            let end = line == "\r\n";
            head.push_str(&line);
            assert!(head.len() <= 8192);
            if end {
                break;
            }
        }
        let status = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let chunked = head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked");
        Self {
            reader,
            status,
            head,
            chunked,
        }
    }

    pub(super) async fn raw(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = self.reader.read(&mut buffer).await.unwrap();
            if count == 0 {
                return bytes;
            }
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 64 * 1024);
        }
    }

    pub(super) async fn body(mut self) -> Vec<u8> {
        let bytes = self.raw().await;
        if !self.chunked {
            return bytes;
        }
        let mut encoded = bytes.as_slice();
        let mut decoded = Vec::new();
        loop {
            let end = encoded.windows(2).position(|part| part == b"\r\n").unwrap();
            let size =
                usize::from_str_radix(std::str::from_utf8(&encoded[..end]).unwrap(), 16).unwrap();
            encoded = &encoded[end + 2..];
            if size == 0 {
                assert_eq!(encoded, b"\r\n");
                return decoded;
            }
            assert!(size <= encoded.len().saturating_sub(2));
            decoded.extend_from_slice(&encoded[..size]);
            assert_eq!(&encoded[size..size + 2], b"\r\n");
            encoded = &encoded[size + 2..];
        }
    }

    pub(super) async fn reading(&mut self) -> Value {
        assert!(self.chunked);
        let mut decoded = Vec::new();
        for _ in 0..16 {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            let size = usize::from_str_radix(line.trim(), 16).unwrap();
            assert!((1..=64 * 1024).contains(&size));
            let start = decoded.len();
            decoded.resize(start + size, 0);
            self.reader.read_exact(&mut decoded[start..]).await.unwrap();
            let mut end = [0; 2];
            self.reader.read_exact(&mut end).await.unwrap();
            assert_eq!(&end, b"\r\n");
            assert!(decoded.len() <= 64 * 1024);
            for line in std::str::from_utf8(&decoded).unwrap().lines() {
                if let Some(value) = line.strip_prefix("data: ") {
                    return serde_json::from_str(value).unwrap();
                }
            }
        }
        panic!("the bounded SSE reader did not receive its first data event")
    }
}
