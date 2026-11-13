//! The WebSocket operation accepts the same closed transfer before initializer entry.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::super::harness;
use super::super::observation::install;
use super::super::state::{SourceStep, State};
use super::support::{NEVER_DEADLINES, Registered, run, worker};
use crate::subscription::worker::websocket::{
    WebSocketSubscriptionSetupOutcome, WebSocketValidationOutcome,
};

#[test]
fn websocket_source_entry_follows_outside_handoff_and_retains_the_same_receipt() {
    let probe = State::new(vec![SourceStep::Pending]);
    let host = install(&probe);
    let registered = Registered::new(probe);
    let id: Arc<str> = Arc::from("observation-receipt");
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        harness::request(),
        None,
        registered.begin(id),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("validation must succeed")
    };
    let setup = {
        let _entered = registered.runtime.enter();
        prepared
            .activate_subscription()
            .unwrap_or_else(|_| panic!("active admission must succeed"))
    };
    let WebSocketSubscriptionSetupOutcome::Ready { subscription, .. } =
        run(&registered.runtime, setup)
    else {
        panic!("source must be ready")
    };
    assert_eq!(host.preparations.load(Ordering::SeqCst), 1);
    assert_eq!(host.entries.load(Ordering::SeqCst), 1);
    assert!(host.terminals.lock().unwrap().is_empty());
    drop(subscription);
    registered.drain();
    assert_eq!(host.terminals.lock().unwrap().len(), 1);
}
