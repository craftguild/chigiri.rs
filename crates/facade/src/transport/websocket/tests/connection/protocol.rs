use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use super::super::support::{ManualDeadlines, UPGRADE_TIMEOUT, assert_no_execution};
use super::support::{
    Authorizer, Decision, admitted, endpoint, finish_guard, initialization, poll_once, ready,
    runtime,
};
use crate::transport::websocket::{
    InitializedWebSocketInput, WebSocketAcknowledgementResolution, WebSocketConnectionResolution,
    WebSocketInitializationInput,
};

struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn accept_requires_ack_flush_and_final_frame_drop_before_initialization() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let (endpoint, signals) = super::super::support::endpoint_with_authorizer(
        super::super::support::AdmissionMode::Continue,
        1,
        16 * 1024,
        Authorizer {
            calls: Arc::clone(&calls),
            decision: Decision::Accept,
        },
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    assert_eq!(
        deadlines
            .requested()
            .into_iter()
            .filter(|duration| *duration == UPGRADE_TIMEOUT)
            .count(),
        2
    );
    {
        let _entered = runtime.enter();
        assert_eq!(
            initialization
                .receive_text(r#"{"type":"ping","payload":{"n":1}}"#)
                .unwrap(),
            WebSocketInitializationInput::ControlQueued
        );
        assert_eq!(
            initialization.receive_text(r#"{"type":"pong"}"#).unwrap(),
            WebSocketInitializationInput::PongObserved
        );
        assert_eq!(
            initialization
                .receive_text(r#"{"type":"connection_init"}"#)
                .unwrap(),
            WebSocketInitializationInput::AuthorizationStarted
        );
    }
    let WebSocketConnectionResolution::Acknowledgement(mut acknowledgement) =
        runtime.block_on(initialization)
    else {
        panic!("authorization should accept")
    };
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_no_execution(&signals);
    let (frame, progress) = acknowledgement
        .begin_acknowledgement()
        .unwrap()
        .expect("queued acknowledgement");
    assert_eq!(frame.as_bytes(), br#"{"type":"connection_ack"}"#);
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&wakes));
    let mut context = Context::from_waker(&waker);
    assert!(
        std::pin::Pin::new(&mut acknowledgement)
            .poll(&mut context)
            .is_pending()
    );
    progress.written_and_flushed();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(
        std::pin::Pin::new(&mut acknowledgement)
            .poll(&mut context)
            .is_pending()
    );
    drop(frame);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    let Poll::Ready(WebSocketAcknowledgementResolution::Initialized(mut initialized)) =
        std::pin::Pin::new(&mut acknowledgement).poll(&mut context)
    else {
        panic!("flush and final physical drop must initialize")
    };
    assert_eq!(initialized.initialization_payload_charge(), 0);
    assert!(initialized.initialization_payload().is_none());
    let pong = initialized
        .begin_control_send()
        .unwrap()
        .expect("pre-ack peer pong remains queued after priority ack");
    assert_eq!(pong.as_bytes(), br#"{"type":"pong","payload":{"n":1}}"#);
    drop(pong);
    let mut parts = initialized.into_session_parts_for_test().unwrap();
    assert!(matches!(
        parts
            .receive_text(r#"{"id":"operation","type":"complete"}"#)
            .unwrap(),
        InitializedWebSocketInput::Complete(_)
    ));
    let close = match parts.receive_text(r#"{"type":"connection_init"}"#) {
        Err(close) => close,
        Ok(_) => panic!("a second initialization must close"),
    };
    assert_eq!(close.code(), 4429);
    assert_eq!(parts.receive_binary().code(), 4429);
    let cleanup = parts.into_cleanup();
    assert_eq!(cleanup.close().code(), 4429);
    runtime.block_on(cleanup);
    finish_guard(&runtime, guard);
}

#[test]
fn pre_ack_protocol_failures_use_fixed_close_codes_without_execution() {
    for (message, code) in [
        (
            r#"{"id":"a","type":"subscribe","payload":{"query":"query { value }"}}"#,
            4401,
        ),
        ("{}", 4400),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, signals) = super::super::support::endpoint_with_authorizer(
            super::super::support::AdmissionMode::Continue,
            1,
            16 * 1024,
            Authorizer {
                calls: Arc::clone(&calls),
                decision: Decision::Accept,
            },
        );
        let runtime = runtime();
        let guard = endpoint.__register_subscription_runtime().unwrap();
        let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
        let mut initialization = initialization(&endpoint, deadlines);
        assert_eq!(
            initialization.receive_text(message).unwrap_err().code(),
            code
        );
        let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization)
        else {
            panic!("invalid pre-ack message must close")
        };
        runtime.block_on(cleanup);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_no_execution(&signals);
        finish_guard(&runtime, guard);
    }
}

#[test]
fn binary_init_input_uses_the_fixed_bad_request_close() {
    let runtime = runtime();
    let endpoint = endpoint(
        Authorizer {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    assert_eq!(initialization.receive_binary().code(), 4400);
    let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization) else {
        panic!("binary initialization input must close")
    };
    runtime.block_on(cleanup);
    finish_guard(&runtime, guard);
}

#[test]
fn timer_precedes_ready_authorization_and_ack_failure_never_initializes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint(
        Authorizer {
            calls,
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let runtime = runtime();
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    {
        let _entered = runtime.enter();
        initialization
            .receive_text(r#"{"type":"connection_init"}"#)
            .unwrap();
    }
    deadlines.expire(UPGRADE_TIMEOUT);
    let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization) else {
        panic!("the original-duration initialization timer must win")
    };
    assert_eq!(cleanup.close().code(), 4408);
    runtime.block_on(cleanup);
    finish_guard(&runtime, guard);
}

#[test]
fn expired_upgrade_handoff_still_starts_one_initialization_timer_without_callback() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint(
        Authorizer {
            calls: Arc::clone(&calls),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let claimed = endpoint
        .__claim_websocket_upgrade(admitted(&endpoint, deadlines))
        .unwrap();
    let handoff = endpoint
        .__begin_websocket_connection(claimed, deadlines)
        .unwrap();
    deadlines.expire(UPGRADE_TIMEOUT);
    let crate::transport::websocket::WebSocketConnectionHandoffResolution::Closing(cleanup) =
        ready(handoff)
    else {
        panic!("expired upgrade budget must close before initialization")
    };
    assert_eq!(cleanup.close().code(), 4408);
    assert_eq!(
        deadlines
            .requested()
            .into_iter()
            .filter(|duration| *duration == UPGRADE_TIMEOUT)
            .count(),
        2
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    runtime.block_on(cleanup);
    finish_guard(&runtime, guard);
}

#[test]
fn full_pre_ack_control_queue_fails_ack_staging_into_bounded_cleanup() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let (endpoint, signals) = super::super::support::endpoint_with_authorizer(
        super::super::support::AdmissionMode::Continue,
        1,
        16 * 1024,
        Authorizer {
            calls: Arc::clone(&calls),
            decision: Decision::Accept,
        },
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    for _ in 0..32 {
        assert_eq!(
            initialization.receive_text(r#"{"type":"ping"}"#).unwrap(),
            WebSocketInitializationInput::ControlQueued
        );
    }
    {
        let _entered = runtime.enter();
        initialization
            .receive_text(r#"{"type":"connection_init"}"#)
            .unwrap();
    }
    let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization) else {
        panic!("a full pre-ack control queue must not initialize")
    };
    assert_eq!(cleanup.close().code(), 1011);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(signals.internal_errors.load(Ordering::SeqCst), 1);
    assert_no_execution(&signals);
    runtime.block_on(cleanup);
    admitted(&endpoint, deadlines).cancel();
    finish_guard(&runtime, guard);
}

#[test]
fn dropped_ack_progress_fails_closed_even_when_frame_is_released() {
    let runtime = runtime();
    let endpoint = endpoint(
        Authorizer {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    {
        let _entered = runtime.enter();
        initialization
            .receive_text(r#"{"type":"connection_init"}"#)
            .unwrap();
    }
    let WebSocketConnectionResolution::Acknowledgement(mut acknowledgement) =
        runtime.block_on(initialization)
    else {
        panic!("authorization should accept")
    };
    let (frame, progress) = acknowledgement.begin_acknowledgement().unwrap().unwrap();
    drop(progress);
    drop(frame);
    let Poll::Ready(WebSocketAcknowledgementResolution::Closing(cleanup)) =
        poll_once(&mut acknowledgement)
    else {
        panic!("broken writer must not initialize the connection")
    };
    assert_eq!(cleanup.close().code(), 1011);
    runtime.block_on(cleanup);
    finish_guard(&runtime, guard);
}
