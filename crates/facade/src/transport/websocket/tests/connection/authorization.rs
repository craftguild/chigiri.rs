use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_http::{
    BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError,
};

use super::super::support::ManualDeadlines;
use super::support::{Authorizer, Decision, endpoint, finish_guard, initialization, runtime};
use crate::transport::websocket::WebSocketConnectionResolution;

struct PendingAuthorizer {
    calls: Arc<AtomicUsize>,
    started: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

impl ConnectionInitAuthorizer for PendingAuthorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started
            .lock()
            .unwrap()
            .take()
            .expect("first authorization")
            .send(())
            .unwrap();
        Box::pin(std::future::pending())
    }
}

#[test]
fn reject_and_opaque_failure_map_to_fixed_safe_closes_once() {
    for (decision, code) in [
        (Decision::Reject, 4403),
        (Decision::Error, 1011),
        (Decision::Panic, 1011),
    ] {
        let runtime = runtime();
        let calls = Arc::new(AtomicUsize::new(0));
        let endpoint = endpoint(
            Authorizer {
                calls: Arc::clone(&calls),
                decision,
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
                .receive_text(r#"{"type":"connection_init","payload":null}"#)
                .unwrap();
        }
        let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization)
        else {
            panic!("authorization must close")
        };
        assert_eq!(cleanup.close().code(), code);
        let debug = format!("{:?}", cleanup.close());
        assert!(!debug.contains("private authorization failure"));
        runtime.block_on(cleanup);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        finish_guard(&runtime, guard);
    }
}

#[test]
fn second_init_closes_while_the_single_authorizer_remains_pending() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (endpoint, _) = super::super::support::endpoint_with_authorizer(
        super::super::support::AdmissionMode::Continue,
        1,
        16 * 1024,
        PendingAuthorizer {
            calls: Arc::clone(&calls),
            started: std::sync::Mutex::new(Some(started_tx)),
        },
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
    started_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        initialization
            .receive_text(r#"{"type":"connection_init","payload":{}}"#)
            .unwrap_err()
            .code(),
        4429
    );
    let WebSocketConnectionResolution::Closing(cleanup) = runtime.block_on(initialization) else {
        panic!("second initialization must close")
    };
    assert_eq!(cleanup.close().code(), 4429);
    runtime.block_on(cleanup);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    finish_guard(&runtime, guard);
}

#[test]
fn accepted_init_is_retained_and_charged_without_a_premature_operation_limit() {
    let runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint(
        Authorizer {
            calls,
            decision: Decision::Accept,
        },
        1,
        1024,
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    let private = "x".repeat(900);
    let input = format!(r#"{{"type":"connection_init","payload":{{"private":"{private}"}}}}"#);
    {
        let _entered = runtime.enter();
        initialization.receive_text(&input).unwrap();
    }
    let WebSocketConnectionResolution::Acknowledgement(acknowledgement) =
        runtime.block_on(initialization)
    else {
        panic!("the 16 KiB decoder bound, not an operation limit, applies here")
    };
    drop(acknowledgement);
    finish_guard(&runtime, guard);
}

#[test]
fn missing_explicit_runtime_registration_fails_before_authorizer_poll() {
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint(
        Authorizer {
            calls: Arc::clone(&calls),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
    let mut initialization = initialization(&endpoint, deadlines);
    assert_eq!(
        initialization
            .receive_text(r#"{"type":"connection_init"}"#)
            .unwrap_err()
            .code(),
        1011
    );
    let WebSocketConnectionResolution::Closing(cleanup) = super::support::ready(initialization)
    else {
        panic!("worker startup failure must close")
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    super::support::ready(cleanup);
}
