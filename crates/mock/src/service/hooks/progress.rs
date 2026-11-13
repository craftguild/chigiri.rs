//! Track skipped after-hooks and unfinished futures in a fixed three-phase structure.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::http::Trace;
use crate::hooks::HookSlot;
use crate::service::Client;

pub(super) struct After {
    trace: Arc<Trace>,
    pending: u8,
}

impl After {
    pub(super) fn new(trace: &Arc<Trace>, client: &Client, slot: HookSlot) -> Self {
        Self {
            trace: trace.clone(),
            pending: if client.enabled(slot) { bit(slot) } else { 0 },
        }
    }

    /// Mark that execution reached this after-hook slot, regardless of its eventual outcome.
    pub(super) fn reached(&mut self) {
        self.pending = 0;
    }
}

impl Drop for After {
    fn drop(&mut self) {
        self.trace
            .unreached_after
            .fetch_or(self.pending, Ordering::AcqRel);
    }
}

impl Trace {
    pub(super) fn unreached_after_hooks(&self) -> Vec<HookSlot> {
        let bits = self.unreached_after.load(Ordering::Acquire);
        [
            HookSlot::FieldAfter,
            HookSlot::OperationAfter,
            HookSlot::RequestAfter,
        ]
        .into_iter()
        .filter(|slot| bits & bit(*slot) != 0)
        .collect()
    }
}

fn bit(slot: HookSlot) -> u8 {
    match slot {
        HookSlot::FieldAfter => 1,
        HookSlot::OperationAfter => 2,
        HookSlot::RequestAfter => 4,
        _ => 0,
    }
}
