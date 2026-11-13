use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub fn request(address: SocketAddr, accept: &str, document: &str) -> HttpResponse {
    let mut stream = send_without_reading(address, accept, document);
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read loopback response");
    parse_response(&response)
}

pub fn send_without_reading(address: SocketAddr, accept: &str, document: &str) -> TcpStream {
    let body = serde_json::json!({ "query": document }).to_string();
    let request = format!(
        "POST /graphql HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .expect("connect loopback server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set response timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("set request timeout");
    stream
        .write_all(request.as_bytes())
        .expect("write loopback request");
    stream
}

pub fn drain_after_pressure(stream: &mut TcpStream) -> HttpResponse {
    let mut buffer = [0; 8192];
    let mut response = Vec::new();
    // This bounded drain cannot trigger the already observed cancellation.
    // It permits observation of the policy terminal before disconnect; an
    // unrelated body/service failure cannot produce a valid full response.
    for _ in 0..8192 {
        match stream.read(&mut buffer) {
            Ok(0) => return parse_response(&response),
            Ok(count) => response.extend_from_slice(&buffer[..count]),
            Err(error) => panic!("drain after observed pressure cancellation: {error}"),
        }
    }
    panic!("bounded post-cancellation drain exceeded 64 MiB");
}

fn parse_response(response: &[u8]) -> HttpResponse {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response head terminator");
    let head = std::str::from_utf8(&response[..boundary]).expect("ASCII response head");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .expect("HTTP response status");
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("HTTP response header");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect::<BTreeMap<_, _>>();
    let encoded = &response[boundary + 4..];
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        decode_chunked(encoded)
    } else {
        encoded.to_vec()
    };
    HttpResponse {
        status,
        headers,
        body,
    }
}

fn decode_chunked(mut input: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size terminator");
        let size_text = std::str::from_utf8(&input[..line_end])
            .expect("ASCII chunk size")
            .split(';')
            .next()
            .expect("chunk size");
        let size = usize::from_str_radix(size_text, 16).expect("hexadecimal chunk size");
        input = &input[line_end + 2..];
        if size == 0 {
            break;
        }
        assert!(input.len() >= size + 2, "complete chunk body");
        decoded.extend_from_slice(&input[..size]);
        assert_eq!(&input[size..size + 2], b"\r\n");
        input = &input[size + 2..];
    }
    decoded
}
