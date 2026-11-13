use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chigiri_http::{
    BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError,
};

use super::super::support::{ManualDeadlines, UPGRADE_TIMEOUT, finite, invoke, valid_headers};
use super::support::{
    Authorizer, Decision, admitted, endpoint, finish_guard, initialization, poll_once, runtime,
};
use crate::transport::websocket::{WebSocketConnectionResolution, WebSocketSeedError};

struct BlockingAuthorizer {
    calls: Arc<AtomicUsize>,
    entered: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl ConnectionInitAuthorizer for BlockingAuthorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let entered = self.entered.lock().unwrap().take();
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            entered.expect("first authorization").send(()).unwrap();
            let (lock, signal) = &*gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = signal.wait(released).unwrap();
            }
            Ok(ConnectionInitDecision::Accept)
        })
    }
}

#[test]
fn blocked_authorization_times_out_outside_the_worker_and_retains_quota_until_join() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (endpoint, _) = super::super::support::endpoint_with_authorizer(
        super::super::support::AdmissionMode::Continue,
        1,
        16 * 1024,
        BlockingAuthorizer {
            calls: Arc::clone(&calls),
            entered: Mutex::new(Some(entered_tx)),
            gate: Arc::clone(&gate),
        },
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    {
        let _entered = runtime.enter();
        initialization
            .receive_text(r#"{"type":"connection_init","payload":{}}"#)
            .unwrap();
    }
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(poll_once(&mut initialization).is_pending());
    deadlines.expire(UPGRADE_TIMEOUT);
    let std::task::Poll::Ready(WebSocketConnectionResolution::Closing(cleanup)) =
        poll_once(&mut initialization)
    else {
        panic!("external original-duration timer must close blocked authorization")
    };
    assert_eq!(cleanup.close().code(), 4408);

    let capacity = finite(invoke(
        &endpoint,
        &ManualDeadlines::default(),
        "GET",
        &valid_headers(true),
        &chigiri_http::ApplicationMetadata::default(),
    ));
    assert_eq!(capacity.status, 503);
    drop(cleanup);
    let capacity = finite(invoke(
        &endpoint,
        &ManualDeadlines::default(),
        "GET",
        &valid_headers(true),
        &chigiri_http::ApplicationMetadata::default(),
    ));
    assert_eq!(capacity.status, 503);

    let (lock, signal) = &*gate;
    *lock.lock().unwrap() = true;
    signal.notify_all();
    let cleanup_driver = endpoint.__subscription_cleanup_driver();
    runtime
        .block_on(std::future::poll_fn(|context| {
            cleanup_driver.poll_one(context)
        }))
        .unwrap();
    admitted(&endpoint, Box::leak(Box::new(ManualDeadlines::default()))).cancel();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    finish_guard(&runtime, guard);
}

#[test]
fn foreign_begin_consumes_the_claimed_seed_without_timer_or_callback() {
    let runtime = runtime();
    let issuer_calls = Arc::new(AtomicUsize::new(0));
    let foreign_calls = Arc::new(AtomicUsize::new(0));
    let issuer = endpoint(
        Authorizer {
            calls: Arc::clone(&issuer_calls),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let foreign = endpoint(
        Authorizer {
            calls: Arc::clone(&foreign_calls),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let issuer_guard = issuer.__register_subscription_runtime().unwrap();
    let foreign_guard = foreign.__register_subscription_runtime().unwrap();
    let issuer_deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let claimed = issuer
        .__claim_websocket_upgrade(admitted(&issuer, issuer_deadlines))
        .unwrap();
    let foreign_deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    assert!(matches!(
        foreign.__begin_websocket_connection(claimed, foreign_deadlines),
        Err(WebSocketSeedError::ForeignEndpoint)
    ));
    assert!(foreign_deadlines.requested().is_empty());
    assert_eq!(issuer_calls.load(Ordering::SeqCst), 0);
    assert_eq!(foreign_calls.load(Ordering::SeqCst), 0);
    admitted(&issuer, Box::leak(Box::new(ManualDeadlines::default()))).cancel();
    finish_guard(&runtime, issuer_guard);
    finish_guard(&runtime, foreign_guard);
}

#[test]
fn writer_lease_keeps_connection_capacity_after_logical_cleanup() {
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
    let WebSocketConnectionResolution::Acknowledgement(mut ack) = runtime.block_on(initialization)
    else {
        panic!("authorization should accept")
    };
    let (frame, progress) = ack.begin_acknowledgement().unwrap().unwrap();
    progress.written_and_flushed();
    drop(ack);
    let cleanup_driver = endpoint.__subscription_cleanup_driver();
    runtime
        .block_on(std::future::poll_fn(|context| {
            cleanup_driver.poll_one(context)
        }))
        .unwrap();
    assert_eq!(
        finite(invoke(
            &endpoint,
            &ManualDeadlines::default(),
            "GET",
            &valid_headers(true),
            &chigiri_http::ApplicationMetadata::default(),
        ))
        .status,
        503
    );
    drop(frame);
    admitted(&endpoint, Box::leak(Box::new(ManualDeadlines::default()))).cancel();
    finish_guard(&runtime, guard);
}
