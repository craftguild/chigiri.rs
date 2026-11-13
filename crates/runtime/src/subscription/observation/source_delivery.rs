//! Fixed host-only source loss shared with final encoded delivery accounting.

use std::sync::{Arc, Mutex};

const MAX_EXACT: u64 = (1_u64 << 53) - 1;

/// Bounded source metadata; it contains no event, callback, credentials or context.
#[doc(hidden)]
pub struct HostSourceDelivery {
    state: Mutex<State>,
    _retention: Option<Arc<dyn Send + Sync>>,
}

#[derive(Default)]
struct State {
    dropped: u64,
    stopped: bool,
}

impl HostSourceDelivery {
    /// Largest cumulative delivery count represented exactly by JSON clients.
    pub const MAX_DROPPED_TOTAL: u64 = MAX_EXACT;
    /// Creates the one counter owned by a host source registration.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            _retention: None,
        })
    }

    /// Retains only a prepaid host accounting token through the last reader.
    /// Hosts must never pass an event, callback or execution context here.
    pub fn retained(retention: Arc<dyn Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            _retention: Some(retention),
        })
    }

    /// Bytes reserved by the host before allocating this shared metadata.
    pub const fn allocation_bytes() -> usize {
        std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()
    }

    /// Advances once for an actual wholly unprocessed source eviction.
    pub fn record_drop(&self) -> Option<u64> {
        let mut state = self.state.lock().ok()?;
        if state.stopped {
            return None;
        }
        match state
            .dropped
            .checked_add(1)
            .filter(|next| *next <= MAX_EXACT)
        {
            Some(next) => {
                state.dropped = next;
                Some(next)
            }
            None => {
                state.stopped = true;
                None
            }
        }
    }

    /// Samples the cumulative source count without freezing later emissions.
    pub fn snapshot(&self) -> Option<u64> {
        Some(self.state.lock().ok()?.dropped)
    }

    /// Stops further source admission and freezes the terminal source count.
    pub fn freeze(&self) -> Option<u64> {
        let mut state = self.state.lock().ok()?;
        state.stopped = true;
        Some(state.dropped)
    }

    /// Returns true after terminal selection or corrupted metadata ownership.
    pub fn is_stopped(&self) -> bool {
        self.state.lock().map_or(true, |state| state.stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_counter_is_exact_and_freeze_prevents_later_evictions() {
        let source = HostSourceDelivery::new();
        assert_eq!(source.record_drop(), Some(1));
        assert_eq!(source.snapshot(), Some(1));
        assert_eq!(source.freeze(), Some(1));
        assert_eq!(source.record_drop(), None);
        assert_eq!(source.snapshot(), Some(1));
        assert!(source.is_stopped());
    }

    #[test]
    fn source_counter_exhaustion_never_wraps_or_loses_json_precision() {
        let source = HostSourceDelivery::new();
        source.state.lock().unwrap().dropped = MAX_EXACT - 1;
        assert_eq!(source.record_drop(), Some(MAX_EXACT));
        assert_eq!(source.record_drop(), None);
        assert_eq!(source.freeze(), Some(MAX_EXACT));
    }
}
