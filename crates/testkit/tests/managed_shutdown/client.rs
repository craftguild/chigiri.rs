//! Bounded native HTTP reads retain the TCP owner in the request future.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct Response {
    pub(super) status: hyper::StatusCode,
    pub(super) body: Vec<u8>,
}

pub(super) async fn request(
    address: SocketAddr,
    path: &str,
    document: Option<&str>,
) -> Result<Response, String> {
    request_with_headers(address, path, document, "").await
}

pub(super) async fn request_with_headers(
    address: SocketAddr,
    path: &str,
    document: Option<&str>,
    headers: &str,
) -> Result<Response, String> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .map_err(|error| error.to_string())?;
        let body = document
            .map(|document| serde_json::json!({"query": document}).to_string())
            .unwrap_or_default();
        let method = if document.is_some() { "POST" } else { "GET" };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: application/graphql-response+json\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream
                .read(&mut buffer)
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            if bytes.len() + count > 64 * 1024 {
                return Err("native fixture response exceeded its bounded read".to_owned());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        Ok(parse(&bytes))
    })
    .await
    .map_err(|_| "owned HTTP exchange did not finish".to_owned())?
}

pub(super) fn parse(bytes: &[u8]) -> Response {
    let boundary = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&bytes[..boundary]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().split_whitespace().nth(1).unwrap();
    let status = hyper::StatusCode::from_u16(status.parse().unwrap()).unwrap();
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').unwrap();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            assert!(value.trim().eq_ignore_ascii_case("chunked"));
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let encoded = &bytes[boundary + 4..];
    let body = if chunked {
        assert!(length.is_none());
        decode_chunked(encoded)
    } else {
        assert_eq!(length, Some(encoded.len()));
        encoded.to_vec()
    };
    Response { status, body }
}

fn decode_chunked(mut input: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    loop {
        let end = input.windows(2).position(|part| part == b"\r\n").unwrap();
        let size = std::str::from_utf8(&input[..end])
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let size = usize::from_str_radix(size, 16).unwrap();
        input = &input[end + 2..];
        if size == 0 {
            assert_eq!(input, b"\r\n");
            return decoded;
        }
        assert!(size <= input.len().saturating_sub(2));
        decoded.extend_from_slice(&input[..size]);
        assert_eq!(&input[size..size + 2], b"\r\n");
        input = &input[size + 2..];
    }
}
