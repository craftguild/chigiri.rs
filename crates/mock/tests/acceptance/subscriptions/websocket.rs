//! Bounded raw WebSocket consumer for the pinned GraphQL transport protocol.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use super::assertions::{Goal, Profile, StreamReport, assert_safe_source_error};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_SOURCE_EVENTS: usize = 512;

pub(super) fn observe(
    address: SocketAddr,
    goal: Goal,
    document: &str,
    ready: Sender<Profile>,
    active: Sender<Profile>,
) -> StreamReport {
    let mut socket = start(address, document);
    ready.send(Profile::WebSocket).unwrap();
    let mut report = StreamReport::new(Profile::WebSocket);
    let mut active_sent = false;
    while !report.done(goal) {
        let message = next_json(&mut socket);
        match message["type"].as_str() {
            Some("next") => {
                assert_eq!(message["id"], "mock");
                let id = report.observe_event(&message["payload"]);
                if id.is_some_and(|id| id.starts_with("warmup")) && !active_sent {
                    active.send(Profile::WebSocket).unwrap();
                    active_sent = true;
                }
            }
            Some("ping") => send_json(
                &mut socket,
                &serde_json::json!({"type":"pong","payload":message.get("payload")}),
            ),
            Some("error") => {
                report.observe_transport_error(&message["payload"]);
                report.mark_terminal();
            }
            Some("complete") => panic!("completion cannot replace a protocol error"),
            kind => panic!("unexpected protocol message {kind:?}: {message}"),
        }
    }
    if matches!(goal, Goal::Recovery) {
        send_json(
            &mut socket,
            &serde_json::json!({"id":"mock","type":"complete"}),
        );
    }
    report
}

pub(super) fn hold(address: SocketAddr, document: &str) -> HeldWebSocket {
    HeldWebSocket(start(address, document))
}

pub(super) struct HeldWebSocket(BufReader<TcpStream>);

impl HeldWebSocket {
    pub(super) fn assert_complete(mut self) {
        for _ in 0..32 {
            let message = next_json(&mut self.0);
            match message["type"].as_str() {
                Some("complete") => {
                    assert_eq!(message["id"], "mock");
                    return;
                }
                Some("ping") => send_json(
                    &mut self.0,
                    &serde_json::json!({"type":"pong","payload":message.get("payload")}),
                ),
                kind => panic!("expected source completion, got {kind:?}: {message}"),
            }
        }
        panic!("bounded control work exceeded before completion");
    }

    pub(super) fn next_changed(&mut self) -> Value {
        for _ in 0..32 {
            let message = next_json(&mut self.0);
            match message["type"].as_str() {
                Some("next") => {
                    assert_eq!(message["id"], "mock");
                    assert!(
                        message["payload"].get("errors").is_none(),
                        "unexpected event error: {message}"
                    );
                    return message["payload"]["data"]["changed"].clone();
                }
                Some("ping") => send_json(
                    &mut self.0,
                    &serde_json::json!({"type":"pong","payload":message.get("payload")}),
                ),
                kind => panic!("unexpected protocol message {kind:?}: {message}"),
            }
        }
        panic!("bounded control work exceeded before event")
    }

    pub(super) fn assert_active(mut self, expected_id: &str, expected_value: Option<i64>) -> Self {
        let message = next_json(&mut self.0);
        assert_eq!(message["type"], "next");
        assert_eq!(message["id"], "mock");
        let changed = &message["payload"]["data"]["changed"];
        assert_eq!(changed["id"], expected_id);
        if let Some(expected_value) = expected_value {
            assert_eq!(changed["value"], expected_value);
        } else {
            assert_eq!(changed["padding"], "");
        }
        if let Some(related) = changed.get("related") {
            assert_eq!(related["label"], format!("current-{expected_id}"));
        }
        self
    }

    pub(super) fn assert_source_failure(mut self) -> Vec<String> {
        let mut ids = Vec::new();
        for _ in 0..=MAX_SOURCE_EVENTS {
            let message = next_json(&mut self.0);
            match message["type"].as_str() {
                Some("next") => {
                    assert_eq!(message["id"], "mock");
                    let changed = &message["payload"]["data"]["changed"];
                    let id = changed["id"].as_str().expect("whole source event");
                    assert_eq!(changed["padding"].as_str().unwrap().len(), 4 * 1024);
                    ids.push(id.to_owned());
                }
                Some("error") => {
                    assert_eq!(message["id"], "mock");
                    assert_safe_source_error(&message["payload"]);
                    return ids;
                }
                kind => panic!("unexpected source terminal message {kind:?}: {message}"),
            }
        }
        panic!("source terminal exceeded its bounded event prefix")
    }

    pub(super) fn complete(mut self) {
        send_json(
            &mut self.0,
            &serde_json::json!({"id":"mock","type":"complete"}),
        );
    }

    pub(super) fn assert_closed(mut self) {
        self.0
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut total = 0;
        let mut bytes = [0; 4096];
        loop {
            match self.0.read(&mut bytes) {
                Ok(0) => return,
                Ok(count) => {
                    total += count;
                    assert!(
                        total <= MAX_FRAME,
                        "shutdown WebSocket output remained bounded"
                    );
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
                Err(error) => panic!("live WebSocket did not close: {error}"),
            }
        }
    }
}

fn start(address: SocketAddr, document: &str) -> BufReader<TcpStream> {
    let mut socket = connect(address);
    send_json(&mut socket, &serde_json::json!({"type":"connection_init"}));
    let acknowledgement = next_json(&mut socket);
    assert_eq!(
        acknowledgement,
        serde_json::json!({"type":"connection_ack"})
    );
    send_json(
        &mut socket,
        &serde_json::json!({
            "id":"mock",
            "type":"subscribe",
            "payload":{"query":document}
        }),
    );
    socket
}

fn connect(address: SocketAddr) -> BufReader<TcpStream> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /graphql HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: graphql-transport-ws\r\nX-Chigiri-Preflight: 1\r\n\r\n"
    )
    .unwrap();
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    assert!(status.starts_with("HTTP/1.1 101 "), "{status:?}");
    let mut protocol = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim_end().split_once(':').unwrap();
        if name.eq_ignore_ascii_case("sec-websocket-protocol") {
            assert_eq!(value.trim(), "graphql-transport-ws");
            protocol = true;
        }
        assert!(!name.eq_ignore_ascii_case("content-length"));
    }
    assert!(protocol);
    reader
}

fn send_json(socket: &mut BufReader<TcpStream>, value: &Value) {
    send_frame(socket.get_mut(), 1, value.to_string().as_bytes());
}

fn next_json(socket: &mut BufReader<TcpStream>) -> Value {
    for _ in 0..32 {
        let (opcode, payload) = receive_frame(socket);
        match opcode {
            1 => return serde_json::from_slice(&payload).unwrap(),
            8 => panic!("server closed before recovery"),
            9 => send_frame(socket.get_mut(), 10, &payload),
            10 => {}
            _ => panic!("unexpected WebSocket opcode {opcode}"),
        }
    }
    panic!("bounded control work exceeded")
}

fn send_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
    assert!(payload.len() <= u16::MAX.into());
    let mut frame = Vec::with_capacity(payload.len() + 8);
    frame.push(0x80 | opcode);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    let mask = [1_u8, 2, 3, 4];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    stream.write_all(&frame).unwrap();
}

fn receive_frame(reader: &mut BufReader<TcpStream>) -> (u8, Vec<u8>) {
    let mut head = [0; 2];
    reader.read_exact(&mut head).unwrap();
    assert_ne!(head[0] & 0x80, 0, "server frame must be final");
    assert_eq!(head[1] & 0x80, 0, "server frame must be unmasked");
    let opcode = head[0] & 0x0f;
    let mut length = usize::from(head[1] & 0x7f);
    if length == 126 {
        let mut encoded = [0; 2];
        reader.read_exact(&mut encoded).unwrap();
        length = usize::from(u16::from_be_bytes(encoded));
    } else if length == 127 {
        let mut encoded = [0; 8];
        reader.read_exact(&mut encoded).unwrap();
        length = usize::try_from(u64::from_be_bytes(encoded)).unwrap();
    }
    assert!(length <= MAX_FRAME, "bounded server frame");
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).unwrap();
    (opcode, payload)
}
