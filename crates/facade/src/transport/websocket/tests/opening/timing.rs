use super::super::support::{AdmissionMode, assert_no_execution, endpoint};
use super::support::{
    Clock, Socket, flush, poll_once, queue_init, runtime, start, until_frame, until_ready_wait,
};
use crate::transport::websocket::{
    WebSocketInbound, WebSocketOpeningOutcome, WebSocketWriteProgress,
};

#[test]
fn only_positive_current_payload_progress_restarts_the_original_writer_budget() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let clock = Box::leak(Box::new(Clock::default()));
    let socket = Socket::default();
    queue_init(&socket);
    let mut driver = start(&endpoint, clock, socket.clone());
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
    clock.advance_to(9);
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
    clock.advance_to(11);
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    clock.advance_to(18);
    socket
        .0
        .lock()
        .unwrap()
        .incoming
        .push_back(Ok(Some(WebSocketInbound::Text(
            r#"{"type":"pong"}"#.to_owned(),
        ))));
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    // Expiry wins even if a successful flush becomes ready in the same poll.
    clock.advance_to(19);
    flush(&socket, length - 1);
    assert!(matches!(
        runtime.block_on(driver),
        WebSocketOpeningOutcome::Closed
    ));
    assert_eq!(socket.0.lock().unwrap().writes.len(), 1);
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn acknowledgement_readiness_wait_is_already_charged_and_independently_timed() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let clock = Box::leak(Box::new(Clock::default()));
    let socket = Socket::default();
    socket.0.lock().unwrap().ready_pending = true;
    queue_init(&socket);
    let mut driver = start(&endpoint, clock, socket.clone());
    runtime.block_on(until_ready_wait(&mut driver, &socket));
    assert!(socket.0.lock().unwrap().sent.is_empty());
    let calls = socket.0.lock().unwrap().ready_polls;
    clock.advance_to(10);
    assert!(matches!(
        runtime.block_on(driver),
        WebSocketOpeningOutcome::Closed
    ));
    assert_eq!(socket.0.lock().unwrap().ready_polls, calls);
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn initialization_budget_starts_at_actual_socket_handoff_not_first_poll() {
    let runtime = runtime();
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let clock = Box::leak(Box::new(Clock::default()));
    let socket = Socket::default();
    let mut driver = start(&endpoint, clock, socket.clone());
    // The upgraded socket is already handed over at zero; delayed application
    // polling must not grant another full initialization interval.
    clock.advance_to(9);
    assert!(
        runtime
            .block_on(async { poll_once(&mut driver) })
            .is_pending()
    );
    // The original upgrade timer was consumed at nine. This expiration must
    // therefore come from the unchanged initialization timer, not that budget.
    clock.advance_to(10);
    queue_init(&socket);
    assert!(matches!(
        runtime.block_on(async { poll_once(&mut driver) }),
        std::task::Poll::Ready(WebSocketOpeningOutcome::Closed)
    ));
    assert_eq!(socket.0.lock().unwrap().closes, [Some(4408)]);
    assert_eq!(socket.0.lock().unwrap().incoming.len(), 1);
    assert!(socket.0.lock().unwrap().sent.is_empty());
    assert_no_execution(&signals);
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}
