//! Verifies the completeness of received HTTP responses independently of how
//! the connection terminates.

use super::{RESPONSE_BYTES, read_response};
use std::io::{self, Cursor, Read};

struct ResetAfter<'a>(Cursor<&'a [u8]>);

impl Read for ResetAfter<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0.position() == self.0.get_ref().len() as u64 {
            return Err(io::ErrorKind::ConnectionReset.into());
        }
        self.0.read(buffer)
    }
}

fn reset(response: &str, method: &str) -> io::Result<super::Response> {
    read_response(ResetAfter(Cursor::new(response.as_bytes())), method)
}

#[test]
fn complete_response_before_reset_keeps_status_headers_and_actual_utf8_body() {
    let response = reset("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\ncafé!", "POST").unwrap();
    assert_eq!(response.status, 200);
    assert!(response.headers.contains("content-length: 6"));
    assert_eq!(response.body, "café!");
    assert_eq!(
        reset(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n",
            "POST"
        )
        .unwrap()
        .status,
        403
    );
}

#[test]
fn truncated_headers_or_body_and_extra_bytes_fail_for_reset_and_clean_eof() {
    for wire in [
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nxyz",
    ] {
        assert!(reset(wire, "GET").is_err(), "{wire:?}");
        assert!(read_response(wire.as_bytes(), "GET").is_err(), "{wire:?}");
    }
}

#[test]
fn bodyless_responses_keep_the_actual_empty_wire_assertion() {
    for (method, header) in [
        ("HEAD", "HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n"),
        ("POST", "HTTP/1.1 204 No Content\r\n\r\n"),
        ("GET", "HTTP/1.1 304 Not Modified\r\n\r\n"),
    ] {
        assert!(reset(header, method).unwrap().body.is_empty());
        let unexpected = format!("{header}x");
        assert!(reset(&unexpected, method).is_err());
        assert!(read_response(unexpected.as_bytes(), method).is_err());
    }
}

#[test]
fn ambiguous_invalid_and_excessive_lengths_are_rejected_before_acceptance() {
    for header in [
        "Content-Length: 0\r\nContent-Length: 0",
        "Content-Length: -1",
        "Content-Length: +0",
        "Content-Length: 9999999999999999999999999999999",
        "Content-Length: 9437185",
        "Content-Length: 0\r\nTransfer-Encoding: chunked",
        "Content-Length: 0\r\n Content-Length: 100",
        "Content-Length: 0\r\nContent-Length : 100",
        "Content-Length: 0\r\n: invalid",
        "Content-Length: 0\r\nX-Test: invalid\nvalue",
        "Content-Length: 0\r\nX-Test: invalid\rvalue",
    ] {
        let wire = format!("HTTP/1.1 200 OK\r\n{header}\r\n\r\n");
        assert!(reset(&wire, "GET").is_err(), "{header}");
        assert!(read_response(wire.as_bytes(), "GET").is_err(), "{header}");
    }
    let over = format!("HTTP/1.1 200 OK\r\n\r\n{}", "x".repeat(RESPONSE_BYTES));
    assert!(read_response(over.as_bytes(), "GET").is_err());
}

#[test]
fn unknown_length_needs_clean_eof_and_other_io_errors_remain_failures() {
    let wire = "HTTP/1.1 200 OK\r\n\r\ncomplete only at EOF";
    assert!(reset(wire, "GET").is_err());
    assert!(reset("HTTP/1.1 100 Continue\r\n\r\n", "GET").is_err());
    assert_eq!(
        read_response(wire.as_bytes(), "GET").unwrap().body,
        "complete only at EOF"
    );
    let timed_out = std::io::repeat(0).take(0).chain(Failure);
    assert_eq!(
        read_response(timed_out, "GET").err().unwrap().kind(),
        io::ErrorKind::TimedOut
    );
}

struct Failure;

impl Read for Failure {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::TimedOut.into())
    }
}
