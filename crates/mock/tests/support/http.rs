//! Provide a bounded HTTP harness that sends GraphQL input through the production path.

use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

#[cfg(test)]
mod tests;

const RESPONSE_BYTES: usize = 9 * 1024 * 1024;

pub struct Response {
    pub status: u16,
    pub headers: String,
    pub body: String,
}

pub fn request(
    address: SocketAddr,
    method: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Response {
    request_path(address, method, "/graphql", headers, body)
}

pub fn request_path(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Response {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(7)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let default_host = address.to_string();
    let host = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map_or(default_host.as_str(), |(_, value)| *value);
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("host") {
            continue;
        }
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    read_response(stream, method)
        .unwrap_or_else(|error| panic!("{method} {path} at {address}: {error}"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn read_response(reader: impl Read, method: &str) -> io::Result<Response> {
    let mut received = Vec::new();
    let result = reader
        .take((RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut received);
    let reset = match result {
        Ok(_) => false,
        Err(error) if error.kind() == io::ErrorKind::ConnectionReset => true,
        Err(error) => return Err(error),
    };
    if received.len() > RESPONSE_BYTES {
        return Err(invalid("HTTP response exceeds the receive limit"));
    }
    let response =
        String::from_utf8(received).map_err(|_| invalid("HTTP response is not UTF-8"))?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| invalid("HTTP response headers are incomplete"))?;
    let mut lines = headers.split("\r\n");
    let mut status_line = lines.next().unwrap_or_default().split_whitespace();
    if !matches!(status_line.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(invalid("unsupported HTTP response version"));
    }
    let status = status_line
        .next()
        .filter(|value| value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (200..600).contains(value))
        .ok_or_else(|| invalid("HTTP response status is incorrect."))?;
    let mut length = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("malformed HTTP response header"))?;
        axum::http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| invalid("HTTP response header name is incorrect."))?;
        axum::http::HeaderValue::from_str(value)
            .map_err(|_| invalid("HTTP response header is wrong."))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid(
                "transfer-encoding was mixed into a fixed-length HTTP test.",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if length.is_some()
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid("invalid or duplicate HTTP content length"));
            }
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| invalid("HTTP response is too long."))?,
            );
        }
    }
    // When early rejection races an unread request body and disconnect, accept only a complete response.
    // HEAD responses cannot consume a body on behalf of the caller; remaining wire bytes are an error.
    let bodyless = method == "HEAD" || matches!(status, 204 | 304);
    if bodyless && !body.is_empty()
        || !bodyless && length.is_some_and(|length| body.len() != length)
        || reset && !bodyless && length.is_none()
    {
        return Err(invalid(
            "HTTP response body length is incomplete, excessive, or inconsistent",
        ));
    }
    if reset {
        eprintln!(
            "HTTP response completed: {method} status={status} bytes={} reset=true",
            response.len()
        );
    }
    Ok(Response {
        status,
        headers: headers.to_ascii_lowercase(),
        body: body.to_owned(),
    })
}
