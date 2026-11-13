//! Bounded streaming HTTP consumers for SSE and multipart.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use super::assertions::{Goal, Profile, StreamReport, assert_safe_source_error};

const MAX_BUFFER: usize = 64 * 1024;
const MAX_SOURCE_EVENTS: usize = 512;

pub(super) fn observe(
    address: SocketAddr,
    profile: Profile,
    goal: Goal,
    document: &str,
    ready: Sender<Profile>,
    active: Sender<Profile>,
) -> StreamReport {
    let mut input = open(address, profile, document);
    ready.send(profile).unwrap();
    let mut report = StreamReport::new(profile);
    let mut active_sent = false;
    while !report.done(goal) {
        match input.next(profile) {
            HttpEntry::Message(entry) => {
                let id = if entry.get("payload") == Some(&Value::Null) {
                    report.observe_transport_error(&entry["errors"]);
                    Some("failure")
                } else {
                    let payload = entry.get("payload").unwrap_or(&entry);
                    report.observe_event(payload)
                };
                if id.is_some_and(|id| id.starts_with("warmup")) && !active_sent {
                    active.send(profile).unwrap();
                    active_sent = true;
                }
            }
            HttpEntry::Complete => report.mark_terminal(),
        }
    }
    report
}

pub(super) fn hold(address: SocketAddr, profile: Profile, document: &str) -> HeldHttp {
    HeldHttp {
        input: open(address, profile, document),
        profile,
    }
}

pub(super) struct HeldHttp {
    input: StreamInput,
    profile: Profile,
}

impl HeldHttp {
    pub(super) fn assert_complete(mut self) {
        assert!(matches!(self.input.next(self.profile), HttpEntry::Complete));
    }

    pub(super) fn next_changed(&mut self) -> Value {
        let HttpEntry::Message(entry) = self.input.next(self.profile) else {
            panic!("source completed before its next event")
        };
        assert!(
            entry.get("errors").is_none(),
            "unexpected event error: {entry}"
        );
        let payload = entry.get("payload").unwrap_or(&entry);
        payload
            .pointer("/data/changed")
            .expect("subscription event data")
            .clone()
    }

    pub(super) fn assert_active(mut self, expected_id: &str) -> Self {
        let HttpEntry::Message(entry) = self.input.next(self.profile) else {
            panic!("source completed before its readiness event")
        };
        let payload = entry.get("payload").unwrap_or(&entry);
        assert_eq!(
            payload.pointer("/data/changed/id"),
            Some(&Value::String(expected_id.into()))
        );
        self
    }

    pub(super) fn assert_source_failure(mut self) -> Vec<String> {
        let mut ids = Vec::new();
        for _ in 0..=MAX_SOURCE_EVENTS {
            match self.input.next(self.profile) {
                HttpEntry::Message(entry)
                    if entry.get("payload") == Some(&Value::Null)
                        || entry.get("errors").is_some() =>
                {
                    assert_safe_source_error(&entry["errors"]);
                    assert!(matches!(self.input.next(self.profile), HttpEntry::Complete));
                    return ids;
                }
                HttpEntry::Message(entry) => {
                    let payload = entry.get("payload").unwrap_or(&entry);
                    let changed = &payload["data"]["changed"];
                    let id = changed["id"].as_str().expect("whole source event");
                    assert_eq!(changed["padding"].as_str().unwrap().len(), 4 * 1024);
                    ids.push(id.to_owned());
                }
                HttpEntry::Complete => panic!("source failure requires an error before completion"),
            }
        }
        panic!("source terminal exceeded its bounded event prefix")
    }

    pub(super) fn assert_closed(mut self) {
        self.input
            .reader
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut total = 0;
        let mut bytes = [0; 4096];
        loop {
            match self.input.reader.read(&mut bytes) {
                Ok(0) => return,
                Ok(count) => {
                    total += count;
                    assert!(total <= MAX_BUFFER, "shutdown response remained bounded");
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::UnexpectedEof
                    ) =>
                {
                    return;
                }
                Err(error) => panic!("live HTTP stream did not close: {error}"),
            }
        }
    }
}

fn open(address: SocketAddr, profile: Profile, document: &str) -> StreamInput {
    let accept = match profile {
        Profile::ServerSentEvents => "text/event-stream",
        Profile::Multipart => "multipart/mixed; subscriptionSpec=1.0",
        Profile::WebSocket => unreachable!(),
    };
    let body = serde_json::json!({"query":document}).to_string();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "POST /graphql HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: {accept}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    StreamInput::open(stream, profile)
}

struct StreamInput {
    reader: BufReader<TcpStream>,
    chunked: bool,
    chunk_remaining: usize,
    decoded: Vec<u8>,
}

impl StreamInput {
    fn open(stream: TcpStream, profile: Profile) -> Self {
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        assert!(status.starts_with("HTTP/1.1 200 "), "{status:?}");
        let mut chunked = false;
        let mut content_type = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let (name, value) = line.trim_end().split_once(':').unwrap();
            if name.eq_ignore_ascii_case("transfer-encoding") {
                assert_eq!(value.trim().to_ascii_lowercase(), "chunked");
                chunked = true;
            }
            if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.trim().to_owned());
            }
            assert!(!name.eq_ignore_ascii_case("content-length"));
        }
        let expected = match profile {
            Profile::ServerSentEvents => "text/event-stream; charset=utf-8",
            Profile::Multipart => "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
            Profile::WebSocket => unreachable!(),
        };
        assert_eq!(content_type.as_deref(), Some(expected));
        Self {
            reader,
            chunked,
            chunk_remaining: 0,
            decoded: Vec::new(),
        }
    }

    fn next(&mut self, profile: Profile) -> HttpEntry {
        for _ in 0..4096 {
            let found = match profile {
                Profile::ServerSentEvents => take_sse(&mut self.decoded),
                Profile::Multipart => take_multipart(&mut self.decoded),
                Profile::WebSocket => unreachable!(),
            };
            if let Some(found) = found {
                return found;
            }
            self.read_piece();
            assert!(self.decoded.len() <= MAX_BUFFER, "bounded stream entry");
        }
        panic!("stream entry work exceeded its fixed bound")
    }

    fn read_piece(&mut self) {
        if !self.chunked {
            let mut bytes = [0; 4096];
            let count = self.reader.read(&mut bytes).unwrap();
            assert_ne!(count, 0, "stream closed before terminal");
            self.decoded.extend_from_slice(&bytes[..count]);
            return;
        }
        if self.chunk_remaining == 0 {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let size = line.trim_end().split(';').next().unwrap();
            self.chunk_remaining = usize::from_str_radix(size, 16).unwrap();
            assert_ne!(
                self.chunk_remaining, 0,
                "stream ended without protocol terminal"
            );
        }
        let count = self.chunk_remaining.min(4096);
        let start = self.decoded.len();
        self.decoded.resize(start + count, 0);
        self.reader
            .read_exact(&mut self.decoded[start..start + count])
            .unwrap();
        self.chunk_remaining -= count;
        if self.chunk_remaining == 0 {
            let mut ending = [0; 2];
            self.reader.read_exact(&mut ending).unwrap();
            assert_eq!(&ending, b"\r\n");
        }
    }
}

enum HttpEntry {
    Message(Value),
    Complete,
}

fn take_sse(buffer: &mut Vec<u8>) -> Option<HttpEntry> {
    let end = find(buffer, b"\n\n")?;
    let entry = buffer.drain(..end + 2).collect::<Vec<_>>();
    let entry = std::str::from_utf8(&entry[..entry.len() - 2]).unwrap();
    if entry.starts_with(':') {
        return None;
    }
    let mut lines = entry.lines();
    let event = lines.next()?.strip_prefix("event: ")?;
    if event == "complete" {
        return Some(HttpEntry::Complete);
    }
    assert_eq!(event, "next");
    let data = lines.next()?.strip_prefix("data: ")?;
    Some(HttpEntry::Message(serde_json::from_str(data).unwrap()))
}

fn take_multipart(buffer: &mut Vec<u8>) -> Option<HttpEntry> {
    const BOUNDARY: &[u8] = b"--graphql\r\n";
    const HEAD: &[u8] = b"Content-Type: application/json\r\n\r\n";
    if buffer.starts_with(b"--graphql--") {
        buffer.clear();
        return Some(HttpEntry::Complete);
    }
    if buffer.len() < BOUNDARY.len() || !buffer.starts_with(BOUNDARY) {
        return None;
    }
    if buffer.len() < BOUNDARY.len() + HEAD.len() {
        return None;
    }
    assert_eq!(&buffer[BOUNDARY.len()..BOUNDARY.len() + HEAD.len()], HEAD);
    let start = BOUNDARY.len() + HEAD.len();
    let mut values = serde_json::Deserializer::from_slice(&buffer[start..]).into_iter::<Value>();
    let value = match values.next()? {
        Ok(value) => value,
        Err(error) if error.is_eof() => return None,
        Err(error) => panic!("multipart JSON: {error}"),
    };
    let consumed = start + values.byte_offset();
    buffer.drain(..consumed);
    if buffer.starts_with(b"\r\n") {
        buffer.drain(..2);
    }
    Some(HttpEntry::Message(value))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
