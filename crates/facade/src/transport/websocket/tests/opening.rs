//! Actual neutral socket-prefix fixtures, not complete live-bridge qualification.

mod physical;
mod support;
mod timing;

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::ApplicationMetadata;

use super::support::{
    AdmissionMode, ManualDeadlines, UPGRADE_TIMEOUT, assert_no_execution, endpoint, finite, invoke,
    valid_headers,
};
use crate::transport::websocket::{
    WebSocketInbound, WebSocketIoError, WebSocketOpeningOutcome, WebSocketWriteProgress,
};
use support::{Socket, flush, poll_once, queue_init, runtime, start, until_frame};

#[test]
fn opening_keeps_same_socket_and_requires_flush_plus_last_physical_ack_drop() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    socket.0.lock().unwrap().retain = true;
    queue_init(&socket);
    let mut driver = start(&endpoint, deadlines, socket.clone());
    runtime.block_on(until_frame(&mut driver, &socket));
    let length = socket
        .0
        .lock()
        .unwrap()
        .current
        .as_ref()
        .unwrap()
        .as_bytes()
        .len();
    assert_eq!(
        socket.0.lock().unwrap().sent,
        [br#"{"type":"connection_ack"}"#.to_vec()]
    );
    assert_no_execution(&signals);
    flush(&socket, length);
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    {
        let mut state = socket.0.lock().unwrap();
        assert_eq!(state.retained.as_ref().unwrap().as_bytes().len(), length);
        // An adapter may release its last buffer owner while polling input.
        // A harmless pre-ack completion must not turn that wake into a panic.
        state.drop_retained_on_read = true;
        state.incoming.push_back(Ok(Some(WebSocketInbound::Text(
            r#"{"type":"complete","id":"unknown"}"#.to_owned(),
        ))));
    }
    let WebSocketOpeningOutcome::Acknowledged {
        socket: original,
        connection,
    } = runtime.block_on(driver)
    else {
        panic!("only successful flush and final physical ACK release may acknowledge")
    };
    assert!(Arc::ptr_eq(&socket.0, &original.0));
    assert_no_execution(&signals);
    runtime.block_on(connection.into_cleanup());
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn partial_ack_progress_does_not_initialize_and_invalid_flush_fails_closed() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    queue_init(&socket);
    let mut driver = start(&endpoint, deadlines, socket.clone());
    runtime.block_on(until_frame(&mut driver, &socket));
    socket
        .0
        .lock()
        .unwrap()
        .writes
        .push_back(WebSocketWriteProgress {
            accepted: 1,
            flushed: false,
        });
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    assert_no_execution(&signals);
    flush(&socket, 0);
    assert!(matches!(
        runtime.block_on(driver),
        WebSocketOpeningOutcome::Closed
    ));
    assert_eq!(socket.0.lock().unwrap().closes, [None]);
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn stalled_ack_is_timed_without_another_message_and_shutdown_is_bounded() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    queue_init(&socket);
    let mut driver = start(&endpoint, deadlines, socket.clone());
    runtime.block_on(until_frame(&mut driver, &socket));
    socket.0.lock().unwrap().close_pending = true;
    deadlines.expire(Duration::from_secs(10));
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    assert_eq!(socket.0.lock().unwrap().closes, [None]);
    deadlines.expire(Duration::from_secs(5));
    assert!(matches!(
        runtime.block_on(driver),
        WebSocketOpeningOutcome::Closed
    ));
    assert_no_execution(&signals);
    // The test handle is intentionally a last physical buffer reference.
    let frame = socket.0.lock().unwrap().current.take();
    drop(frame);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn original_upgrade_timeout_wins_before_input_or_authorization() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    queue_init(&socket);
    let driver = start(&endpoint, deadlines, socket.clone());
    deadlines.expire(UPGRADE_TIMEOUT);
    assert!(matches!(
        runtime.block_on(driver),
        WebSocketOpeningOutcome::Closed
    ));
    assert_eq!(socket.0.lock().unwrap().closes, [Some(4408)]);
    assert!(socket.0.lock().unwrap().sent.is_empty());
    assert_eq!(
        deadlines
            .requested()
            .iter()
            .filter(|time| **time == UPGRADE_TIMEOUT)
            .count(),
        2
    );
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn malformed_binary_and_pre_ack_subscribe_use_authoritative_safe_close() {
    for (input, expected) in [
        (Ok(Some(WebSocketInbound::Binary)), 4400),
        (Err(WebSocketIoError::Protocol), 4400),
        (
            Ok(Some(WebSocketInbound::Text(
                r#"{"type":"subscribe","id":"x","payload":{"query":"query { value }"}}"#.to_owned(),
            ))),
            4401,
        ),
    ] {
        let runtime = runtime();
        let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
        let guard = endpoint.__register_subscription_runtime().unwrap();
        let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
        let socket = Socket::default();
        socket.0.lock().unwrap().incoming.push_back(input);
        let driver = start(&endpoint, deadlines, socket.clone());
        assert!(matches!(
            runtime.block_on(driver),
            WebSocketOpeningOutcome::Closed
        ));
        assert_eq!(socket.0.lock().unwrap().closes, [Some(expected)]);
        assert_no_execution(&signals);
        runtime.block_on(guard.shutdown().unwrap()).unwrap();
    }
}

#[test]
fn physical_ack_clone_retains_connection_quota_after_driver_cancellation() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    socket.0.lock().unwrap().retain = true;
    queue_init(&socket);
    let mut driver = start(&endpoint, deadlines, socket.clone());
    runtime.block_on(until_frame(&mut driver, &socket));
    let length = socket
        .0
        .lock()
        .unwrap()
        .current
        .as_ref()
        .unwrap()
        .as_bytes()
        .len();
    flush(&socket, length);
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    drop(driver);
    let response = finite(invoke(
        &endpoint,
        deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    ));
    assert_eq!(response.status, 503);
    let frame = socket.0.lock().unwrap().retained.take();
    drop(frame);
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}
