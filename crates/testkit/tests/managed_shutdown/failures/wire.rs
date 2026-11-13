//! Gate the failing event on an actual native HTTP head, with bounded TCP reads.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::{client, fixture::Probe};

pub(super) async fn exchange(
    address: SocketAddr,
    multipart: bool,
    after_head: bool,
    probe: &Probe,
) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let accept = if multipart {
            "multipart/mixed; subscriptionSpec=1.0"
        } else {
            "text/event-stream"
        };
        let body = r#"{"query":"subscription { ticks { value } }"}"#;
        let request = format!(
            "POST /graphql HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "the native head must precede connection termination");
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 64 * 1024);
            if boundary(&bytes).is_some() {
                break;
            }
        }
        let head = head(&bytes);
        assert!(head.starts_with(if after_head { "HTTP/1.1 200 " } else { "HTTP/1.1 500 " }));
        // Event mapping cannot begin before the test has received the status
        // from the real socket, rather than merely preparing an SDK response.
        probe.release();
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 64 * 1024);
        }
        bytes
    })
    .await
    .expect("the owned native failure exchange must finish within its watchdog")
}

pub(super) fn assert_finite(bytes: &[u8]) {
    let head = head(bytes).to_ascii_lowercase();
    assert!(!head.contains("text/event-stream"));
    assert!(!head.contains("multipart/mixed"));
    let response = client::parse(bytes);
    assert_eq!(response.status, hyper::StatusCode::INTERNAL_SERVER_ERROR);
    if !response.body.is_empty() {
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let errors = body["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["extensions"]["code"], "INTERNAL_SERVER_ERROR");
    }
    assert_masked(bytes);
}

pub(super) fn assert_abnormal(bytes: &[u8], multipart: bool) {
    let head = head(bytes).to_ascii_lowercase();
    assert!(head.contains(if multipart {
        "content-type: multipart/mixed; boundary=graphql; subscriptionspec=1.0"
    } else {
        "content-type: text/event-stream; charset=utf-8"
    }));
    assert!(head.contains("transfer-encoding: chunked"));
    assert!(!bytes.ends_with(b"0\r\n\r\n"), "no clean HTTP end chunk");
    let wire = String::from_utf8_lossy(bytes);
    assert!(!wire.contains("event: complete"));
    assert!(!wire.contains("--graphql--"));
    assert_eq!(wire.matches("HTTP/1.1 ").count(), 1);
    assert_masked(bytes);
}

fn assert_masked(bytes: &[u8]) {
    let wire = String::from_utf8_lossy(bytes);
    for private in ["private managed", "preparation failure", "event failure"] {
        assert!(
            !wire.contains(private),
            "no private failure material on the wire"
        );
    }
}

fn boundary(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|part| part == b"\r\n\r\n")
}

fn head(bytes: &[u8]) -> &str {
    std::str::from_utf8(&bytes[..boundary(bytes).unwrap()]).unwrap()
}
