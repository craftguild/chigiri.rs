//! Test-owned context work exercises application hooks without demo fault flags.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use chigiri::{BoxFuture, ContextFactory, HttpRequestContext, PrepareError, PreparedContext};

#[derive(Default)]
pub(super) struct Gate {
    pub(super) entered: AtomicUsize,
    pub(super) setups: AtomicUsize,
    pub(super) errors: AtomicUsize,
    released: AtomicBool,
    changed: tokio::sync::Notify,
}

impl Gate {
    pub(super) fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.changed.notify_one();
    }
}

pub(super) struct Factory {
    pub(super) gate: Arc<Gate>,
    pub(super) hold: bool,
    pub(super) fail_setup: bool,
    pub(super) fail_event: bool,
}

impl ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        request: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        let phase = request.phase();
        Box::pin(async move {
            if matches!(phase, chigiri::RequestPhase::SubscriptionSetup { .. }) {
                self.gate.setups.fetch_add(1, Ordering::SeqCst);
                assert!(!self.fail_setup, "private example setup failure");
            }
            if matches!(phase, chigiri::RequestPhase::SubscriptionEvent { .. }) {
                self.gate.entered.fetch_add(1, Ordering::SeqCst);
                while self.hold && !self.gate.released.load(Ordering::SeqCst) {
                    self.gate.changed.notified().await;
                }
                assert!(!self.fail_event, "private example event failure");
            }
            Ok(PreparedContext::execution_only(()))
        })
    }
}

pub(super) struct Observe(pub(super) Arc<Gate>);

impl chigiri::ErrorObserver for Observe {
    fn observe_error(&self, _: chigiri::ErrorEvent<'_>) {
        self.0.errors.fetch_add(1, Ordering::SeqCst);
    }
}
