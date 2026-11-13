//! Test-only host authority, independent of Mock identities and installed sinks.

use chigiri_runtime::__private::{
    ErasedSubscription, HostSubscriptionObservation, SubscriptionObservationReceipt,
    SubscriptionTermination,
};
use chigiri_runtime::{FieldResult, SubscriptionOverflow};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use super::harness::poll_once;
use super::state::State;

#[derive(Default)]
pub(super) struct Probe {
    pub(super) preparations: AtomicUsize,
    pub(super) entries: AtomicUsize,
    pub(super) terminals: Mutex<Vec<SubscriptionTermination>>,
    pub(super) totals: Mutex<Vec<u64>>,
    pub(super) drops: Mutex<Vec<u64>>,
    pub(super) source_drops_at_finish: Mutex<Vec<usize>>,
    source: Weak<State>,
    receipt: Mutex<Option<SubscriptionObservationReceipt>>,
}

pub(super) fn install(state: &Arc<State>) -> Arc<Probe> {
    let probe = Arc::new(Probe {
        source: Arc::downgrade(state),
        ..Probe::default()
    });
    *state.host_observation.lock().unwrap() = Some(probe.clone());
    probe
}

pub(super) fn prepare(
    state: &State,
    receipt: SubscriptionObservationReceipt,
) -> FieldResult<Option<Box<dyn HostSubscriptionObservation>>> {
    let Some(probe) = state.host_observation.lock().unwrap().clone() else {
        return Ok(None);
    };
    probe.preparations.fetch_add(1, Ordering::SeqCst);
    *probe.receipt.lock().unwrap() = Some(receipt);
    Ok(Some(Box::new(Host(probe))))
}

pub(super) fn entered(state: &State) {
    if let Some(probe) = state.host_observation.lock().unwrap().clone() {
        let receipt = probe.receipt.lock().unwrap().clone().unwrap();
        assert_eq!(
            poll_once(Box::pin(receipt.wait_for_acceptance()).as_mut()),
            std::task::Poll::Ready(true)
        );
        probe.entries.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) fn attach(state: &State, source: ErasedSubscription) -> FieldResult<ErasedSubscription> {
    let receipt = state
        .host_observation
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|probe| probe.receipt.lock().unwrap().clone());
    match receipt {
        Some(receipt) => source.with_host_observation(receipt),
        None => Ok(source),
    }
}

struct Host(Arc<Probe>);
impl HostSubscriptionObservation for Host {
    fn retained_metadata_bytes(&self) -> usize {
        128
    }
    fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        overflow == SubscriptionOverflow::DropOldest
    }
    fn flush(&mut self) {}
    fn drop_oldest(&mut self, total: u64) {
        self.0.drops.lock().unwrap().push(total);
    }
    fn finish(&mut self, decision: SubscriptionTermination, total: u64) {
        if let Some(source) = self.0.source.upgrade() {
            self.0
                .source_drops_at_finish
                .lock()
                .unwrap()
                .push(source.counts.source_drops.load(Ordering::SeqCst));
        }
        self.0.terminals.lock().unwrap().push(decision);
        self.0.totals.lock().unwrap().push(total);
    }
}
