//! Explicit bounded host bytes; allocation follows the offered capacity check.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{
    HostSubscriptionObservation, SubscriptionTermination, invariant_error,
};
use chigiri_runtime::{FieldResult, SubscriptionOverflow};

const PAYLOAD_BYTES: usize = 1024;
pub(crate) const CHARGE: usize = PAYLOAD_BYTES + std::mem::size_of::<Host>();

#[derive(Default)]
pub(crate) struct Probe {
    pub(crate) budgets: Mutex<Vec<usize>>,
    pub(crate) allocated: AtomicUsize,
    pub(crate) dropped: AtomicUsize,
}

pub(crate) fn prepare(
    probe: &Arc<Probe>,
    available: usize,
) -> FieldResult<Box<dyn HostSubscriptionObservation>> {
    probe.budgets.lock().unwrap().push(available);
    if available < CHARGE {
        return Err(invariant_error(
            "host fixture requires its explicit metadata capacity",
        ));
    }
    probe.allocated.fetch_add(1, Ordering::SeqCst);
    Ok(Box::new(Host {
        _payload: Box::new([0; PAYLOAD_BYTES]),
        probe: Arc::clone(probe),
    }))
}

struct Host {
    _payload: Box<[u8; PAYLOAD_BYTES]>,
    probe: Arc<Probe>,
}

impl HostSubscriptionObservation for Host {
    fn retained_metadata_bytes(&self) -> usize {
        CHARGE
    }
    fn flush(&mut self) {}
    fn registered(&self, _: SubscriptionOverflow) -> bool {
        true
    }
    fn drop_oldest(&mut self, _: u64) {}
    fn finish(&mut self, _: SubscriptionTermination, _: u64) {}
}

impl Drop for Host {
    fn drop(&mut self) {
        self.probe.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
