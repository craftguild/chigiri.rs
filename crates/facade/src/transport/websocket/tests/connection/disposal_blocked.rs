//! Cooperative timer disposal must finish before connection recovery is claimed.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, TransportRequest, WebSocketHttpVersion,
};

use super::super::support::valid_headers;
use super::support::{Authorizer, Decision, endpoint, ready};
use crate::transport::websocket::WebSocketUpgradeIngress;

#[derive(Default)]
struct Gate {
    enabled: AtomicBool,
    entered: AtomicBool,
    released: Mutex<bool>,
    changed: Condvar,
}

struct Timers(Arc<Gate>);
struct Timer(Arc<Gate>);

impl DeadlineFactory for Timers {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(Timer(self.0.clone()))
    }
}

impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.0.enabled.load(Ordering::Acquire) {
            let mut released = self.0.released.lock().unwrap();
            self.0.entered.store(true, Ordering::Release);
            self.0.changed.notify_all();
            while !*released {
                released = self.0.changed.wait(released).unwrap();
            }
        }
    }
}

#[test]
fn connection_charge_survives_detach_while_metadata_disposal_is_blocked() {
    let endpoint = endpoint(
        Authorizer {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: Decision::Accept,
        },
        1,
        16 * 1024,
    );
    let guard = endpoint.__register_subscription_runtime().unwrap();
    let gate = Arc::new(Gate::default());
    let timers = Box::leak(Box::new(Timers(gate.clone())));
    let headers = valid_headers(true);
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    let WebSocketUpgradeIngress::Admitted(seed) =
        ready(endpoint.__admit_websocket_upgrade(request, WebSocketHttpVersion::Http11, timers))
    else {
        panic!("valid fixture upgrade must be admitted");
    };
    let claimed = endpoint.__claim_websocket_upgrade(seed).unwrap();
    let handoff = endpoint
        .__begin_websocket_connection(claimed, timers)
        .unwrap();
    gate.enabled.store(true, Ordering::Release);
    let dropper = std::thread::spawn(move || drop(handoff));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let released = gate.released.lock().unwrap();
        let (released, _) = gate
            .changed
            .wait_timeout_while(released, Duration::from_secs(2), |_| {
                !gate.entered.load(Ordering::Acquire)
            })
            .unwrap();
        drop(released);
        assert!(gate.entered.load(Ordering::Acquire));
        drop(guard);
        assert!(
            endpoint.__open_subscription_connection().is_err(),
            "the retained timer still owns unfinished disposal"
        );
    }));
    *gate.released.lock().unwrap() = true;
    gate.changed.notify_all();
    dropper
        .join()
        .expect("owned disposal thread must physically join");
    result.unwrap();
    assert_eq!(endpoint.__subscription_cleanup_driver().pending(), Ok(0));
    assert!(endpoint.__open_subscription_connection().is_ok());
}
