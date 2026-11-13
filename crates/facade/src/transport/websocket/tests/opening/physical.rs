//! Physical release must wake the same opening driver without another input.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Wake, Waker};

use super::super::support::{AdmissionMode, ManualDeadlines, endpoint};
use super::support::{Socket, flush, poll_once, queue_init, runtime, start, until_frame};
use crate::transport::websocket::{WebSocketInbound, WebSocketOpeningOutcome};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn ordinary_control_release_wakes_ack_writer_without_another_peer_message() {
    let runtime = runtime();
    let (endpoint, _) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let socket = Socket::default();
    {
        let mut state = socket.0.lock().unwrap();
        state.retain = true;
        state.incoming.push_back(Ok(Some(WebSocketInbound::Text(
            r#"{"type":"ping"}"#.into(),
        ))));
    }
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
    queue_init(&socket);
    flush(&socket, length);
    let counter = Arc::new(WakeCount::default());
    let waker = Waker::from(Arc::clone(&counter));
    assert!(
        runtime
            .block_on(async { Pin::new(&mut driver).poll(&mut Context::from_waker(&waker)) })
            .is_pending()
    );
    let retained = {
        let mut state = socket.0.lock().unwrap();
        assert!(state.current.is_none());
        assert_eq!(state.sent, [br#"{"type":"pong"}"#.to_vec()]);
        state.retain = false;
        state.retained.take().unwrap()
    };
    let before = counter.0.load(Ordering::SeqCst);
    drop(retained);
    assert!(counter.0.load(Ordering::SeqCst) > before);
    // No queued peer input or forced driver wake is needed to reach the ACK.
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
    let WebSocketOpeningOutcome::Acknowledged { connection, .. } = runtime.block_on(driver) else {
        panic!("released ordinary control must not strand acknowledgement")
    };
    runtime.block_on(connection.into_cleanup());
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}

#[test]
fn external_last_ack_reference_release_finishes_payload_free_writer_wait() {
    let runtime = runtime();
    let (endpoint, _) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
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
    let retained = socket.0.lock().unwrap().retained.take().unwrap();
    drop(retained);
    let WebSocketOpeningOutcome::Acknowledged { connection, .. } = runtime.block_on(driver) else {
        panic!("the ACK proof already includes flush and original payload release")
    };
    runtime.block_on(connection.into_cleanup());
    runtime.block_on(guard.shutdown().unwrap()).unwrap();
}
